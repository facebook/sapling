/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#include "eden/fs/store/LocalStore.h"

using folly::ByteRange;
using folly::Future;
using folly::IOBuf;
using folly::makeFuture;
using folly::makeSemiFuture;
using folly::SemiFuture;
using folly::StringPiece;
using std::make_unique;
using std::string;
using std::unique_ptr;

namespace {

template <typename... Args>
void gitCheckError(int error, Args&&... args) {
  if (error) {
    auto lastError = giterr_last();
    auto message = folly::to<string>(
        std::forward<Args>(args)..., ": ", lastError->message);
    throw std::runtime_error(message);
  }
}

void freeBlobIOBufData(void* /*blobData*/, void* blobObject) {
  git_blob* gitBlob = static_cast<git_blob*>(blobObject);
  git_blob_free(gitBlob);
}
} // namespace

namespace facebook {
namespace eden {

GitBackingStore::GitBackingStore(
    AbsolutePathPiece repository,
    LocalStore* localStore)
    : localStore_{localStore} {
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

Future<unique_ptr<Tree>> GitBackingStore::getTree(const Hash& id) {
  // TODO: Use a separate thread pool to do the git I/O
  return makeFuture(getTreeImpl(id));
}

unique_ptr<Tree> GitBackingStore::getTreeImpl(const Hash& id) {
  XLOG(DBG4) << "importing tree " << id;

  git_oid treeOID = hash2Oid(id);
  git_tree* gitTree = nullptr;
  auto error = git_tree_lookup(&gitTree, repo_, &treeOID);
  gitCheckError(
      error, "unable to find git tree ", id, " in repository ", getPath());
  SCOPE_EXIT {
    git_tree_free(gitTree);
  };

  std::vector<TreeEntry> entries;
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
      throw std::runtime_error(folly::to<string>(
          "unknown file mode ",
          static_cast<int>(entryMode),
          " on file ",
          entryName,
          " in git tree ",
          id));
    }
    auto entryHash = oid2Hash(git_tree_entry_id(gitEntry));
    entries.emplace_back(entryHash, entryName, fileType);
  }
  auto tree = make_unique<Tree>(std::move(entries), id);
  auto hash = localStore_->putTree(tree.get());
  DCHECK_EQ(id, hash);

  return tree;
}

SemiFuture<unique_ptr<Blob>> GitBackingStore::getBlob(const Hash& id) {
  // TODO: Use a separate thread pool to do the git I/O
  return makeSemiFuture(getBlobImpl(id));
}

unique_ptr<Blob> GitBackingStore::getBlobImpl(const Hash& id) {
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

SemiFuture<unique_ptr<Tree>> GitBackingStore::getTreeForCommit(
    const Hash& commitID) {
  // TODO: Use a separate thread pool to do the git I/O
  XLOG(DBG4) << "resolving tree for commit " << commitID;

  // Look up the commit info
  git_oid commitOID = hash2Oid(commitID);
  git_commit* commit = nullptr;
  auto error = git_commit_lookup(&commit, repo_, &commitOID);
  gitCheckError(
      error,
      "unable to find git commit ",
      commitID,
      " in repository ",
      getPath());
  SCOPE_EXIT {
    git_commit_free(commit);
  };

  // Get the tree ID for this commit.
  Hash treeID = oid2Hash(git_commit_tree_id(commit));

  // Now get the specified tree.
  return localStore_->getTree(treeID).thenValue(
      [this, treeID](unique_ptr<Tree> tree) {
        if (tree) {
          return tree;
        } else {
          return getTreeImpl(treeID);
        }
      });
}

SemiFuture<std::unique_ptr<Tree>> GitBackingStore::getTreeForManifest(
    const Hash& commitID,
    const Hash& /* manifestID */) {
  return getTreeForCommit(commitID);
}

git_oid GitBackingStore::hash2Oid(const Hash& hash) {
  git_oid oid;
  static_assert(
      Hash::RAW_SIZE == GIT_OID_RAWSZ,
      "git hash size and eden hash size do not match");
  memcpy(oid.id, hash.getBytes().data(), GIT_OID_RAWSZ);
  return oid;
}

Hash GitBackingStore::oid2Hash(const git_oid* oid) {
  ByteRange oidBytes(oid->id, GIT_OID_RAWSZ);
  return Hash(oidBytes);
}
} // namespace eden
} // namespace facebook
