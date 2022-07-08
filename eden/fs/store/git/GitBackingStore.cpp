/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GitBackingStore.h"

#include <folly/Conv.h>
#include <folly/futures/Future.h>
#include <folly/logging/xlog.h>
#include <git2.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitTree.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/Throw.h"
#include "folly/String.h"

using folly::ByteRange;
using folly::IOBuf;
using folly::makeSemiFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::string;
using std::unique_ptr;

namespace facebook::eden {

namespace {

template <typename... Args>
void gitCheckError(int error, Args&&... args) {
  if (error) {
    auto lastError = giterr_last();
    throw_<std::runtime_error>(
        std::forward<Args>(args)..., ": ", lastError->message);
  }
}

void freeBlobIOBufData(void* /*blobData*/, void* blobObject) {
  git_blob* gitBlob = static_cast<git_blob*>(blobObject);
  git_blob_free(gitBlob);
}

} // namespace

GitBackingStore::GitBackingStore(AbsolutePathPiece repository) {
  // Make sure libgit2 is initialized.
  // (git_libgit2_init() is safe to call multiple times if multiple
  // GitBackingStore objects are created.  git_libgit2_shutdown() should be
  // called once for each call to git_libgit2_init().)
  git_libgit2_init();

  auto error = git_repository_open(&repo_, repository.value().str().c_str());
  gitCheckError(error, "error opening git repository", repository);
}

GitBackingStore::~GitBackingStore() {
  git_repository_free(repo_);
  git_libgit2_shutdown();
}

const char* GitBackingStore::getPath() const {
  return git_repository_path(repo_);
}

RootId GitBackingStore::parseRootId(folly::StringPiece rootId) {
  return RootId{hash20FromThrift(rootId).toString()};
}

std::string GitBackingStore::renderRootId(const RootId& rootId) {
  // In memory, root IDs are stored as 40-byte hex. Thrift clients generally
  // expect 20-byte binary for Mercurial commit hashes, so re-encode that way.
  return folly::unhexlify(rootId.value());
}

ObjectId GitBackingStore::parseObjectId(folly::StringPiece objectId) {
  return ObjectId{hash20FromThrift(objectId).toString()};
}

std::string GitBackingStore::renderObjectId(const ObjectId& objectId) {
  return objectId.asHexString();
}

SemiFuture<unique_ptr<Tree>> GitBackingStore::getRootTree(
    const RootId& rootId,
    ObjectFetchContext& /*context*/) {
  // TODO: Use a separate thread pool to do the git I/O
  XLOG(DBG4) << "resolving tree for commit " << rootId;

  // Look up the commit info
  git_oid commitOID = root2Oid(rootId);
  git_commit* commit = nullptr;
  auto error = git_commit_lookup(&commit, repo_, &commitOID);
  gitCheckError(
      error,
      "unable to find git commit ",
      rootId,
      " in repository ",
      getPath());
  SCOPE_EXIT {
    git_commit_free(commit);
  };

  // Get the tree ID for this commit.
  ObjectId treeID = oid2Hash(git_commit_tree_id(commit));

  // Now get the specified tree.
  return getTreeImpl(treeID);
}

SemiFuture<BackingStore::GetTreeRes> GitBackingStore::getTree(
    const ObjectId& id,
    ObjectFetchContext& /*context*/) {
  // TODO: Use a separate thread pool to do the git I/O
  return makeSemiFuture(BackingStore::GetTreeRes{
      getTreeImpl(id), ObjectFetchContext::Origin::FromDiskCache});
}

unique_ptr<Tree> GitBackingStore::getTreeImpl(const ObjectId& id) {
  XLOG(DBG4) << "importing tree " << id;

  git_oid treeOID = hash2Oid(id);
  git_tree* gitTree = nullptr;
  auto error = git_tree_lookup(&gitTree, repo_, &treeOID);
  gitCheckError(
      error, "unable to find git tree ", id, " in repository ", getPath());
  SCOPE_EXIT {
    git_tree_free(gitTree);
  };

  Tree::container entries{kPathMapDefaultCaseSensitive};
  size_t numEntries = git_tree_entrycount(gitTree);
  for (size_t i = 0; i < numEntries; ++i) {
    auto gitEntry = git_tree_entry_byindex(gitTree, i);
    auto entryMode = git_tree_entry_filemode(gitEntry);
    StringPiece entryName(git_tree_entry_name(gitEntry));
    TreeEntryType fileType;
    if (entryMode == GIT_FILEMODE_TREE) {
      fileType = TreeEntryType::TREE;
    } else if (entryMode == GIT_FILEMODE_BLOB_EXECUTABLE) {
      fileType = TreeEntryType::EXECUTABLE_FILE;
    } else if (entryMode == GIT_FILEMODE_LINK) {
      fileType = TreeEntryType::SYMLINK;
    } else if (entryMode == GIT_FILEMODE_BLOB) {
      fileType = TreeEntryType::REGULAR_FILE;
    } else {
      // TODO: We currently don't handle GIT_FILEMODE_COMMIT
      throw_<std::runtime_error>(
          "unknown file mode ",
          enumValue(entryMode),
          " on file ",
          entryName,
          " in git tree ",
          id);
    }
    auto entryHash = oid2Hash(git_tree_entry_id(gitEntry));
    auto name = PathComponentPiece{entryName};
    entries.emplace(name, entryHash, fileType);
  }
  return make_unique<Tree>(std::move(entries), id);
}

SemiFuture<BackingStore::GetBlobRes> GitBackingStore::getBlob(
    const ObjectId& id,
    ObjectFetchContext& /*context*/) {
  // TODO: Use a separate thread pool to do the git I/O
  return makeSemiFuture(BackingStore::GetBlobRes{
      getBlobImpl(id), ObjectFetchContext::Origin::FromDiskCache});
}

unique_ptr<Blob> GitBackingStore::getBlobImpl(const ObjectId& id) {
  XLOG(DBG5) << "importing blob " << id;

  auto blobOID = hash2Oid(id);
  git_blob* blob = nullptr;
  int error = git_blob_lookup(&blob, repo_, &blobOID);
  gitCheckError(
      error, "unable to find git blob ", id, " in repository ", getPath());

  // Create an IOBuf which points at the blob data owned by git.
  auto dataSize = git_blob_rawsize(blob);
  auto* blobData = git_blob_rawcontent(blob);
  IOBuf buf(
      IOBuf::TAKE_OWNERSHIP,
      const_cast<void*>(blobData),
      dataSize,
      freeBlobIOBufData,
      blob);

  // Create the blob
  return make_unique<Blob>(id, std::move(buf));
}

git_oid GitBackingStore::root2Oid(const RootId& rootId) {
  auto& value = rootId.value();
  CHECK_EQ(40, value.size());
  auto binary = folly::unhexlify(rootId.value());
  CHECK_EQ(GIT_OID_RAWSZ, binary.size());

  git_oid oid;
  memcpy(oid.id, binary.data(), GIT_OID_RAWSZ);
  return oid;
}

git_oid GitBackingStore::hash2Oid(const ObjectId& hash) {
  git_oid oid;
  auto bytes = hash.getBytes();
  CHECK_EQ(bytes.size(), GIT_OID_RAWSZ);
  memcpy(oid.id, bytes.data(), GIT_OID_RAWSZ);
  return oid;
}

ObjectId GitBackingStore::oid2Hash(const git_oid* oid) {
  ByteRange oidBytes(oid->id, GIT_OID_RAWSZ);
  return ObjectId(oidBytes);
}

} // namespace facebook::eden
