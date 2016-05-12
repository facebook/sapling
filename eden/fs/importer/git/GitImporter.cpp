/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "GitImporter.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/LocalStore.h"

#include <folly/Format.h>
#include <folly/ScopeGuard.h>
#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <git2.h>
#include <openssl/sha.h>
#include <iostream>
#include <string>
#include <vector>

using facebook::eden::Hash;
using facebook::eden::LocalStore;
using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::format;
using folly::io::Appender;
using folly::stringPrintf;
using std::string;
using std::unique_ptr;
using std::vector;

/**
 * In Git, the octal representation of the mode for a blob is 100644, 100755, or
 * 120000. For a commit, it is 160000. All of these are 6 octal digits in
 * length.
 */
const int BLOB_OR_COMMIT_MODE_OCTAL_LENGTH = 6;

/**
 * In Git, the octal representation of the mode for a tree is 40000, which is 5
 * octal digits in length.
 */
const int TREE_MODE_OCTAL_LENGTH = 5;

namespace {
struct TreeToExplore {
  explicit TreeToExplore(git_tree* t) : tree(t) {}
  explicit TreeToExplore(git_tree* t, std::unique_ptr<IOBuf> b)
      : tree(t), buf(std::move(b)) {}
  git_tree* tree;
  std::unique_ptr<IOBuf> buf;
};

// Forward declarations.
const string copyGitObjectsToDatabase(git_repository* repo, LocalStore* db);
void writeTreeEntryToDatabase(git_tree* tree, IOBuf* buf, LocalStore* db);
Hash hashForOid(const git_oid* oid);
void addChildrenToStack(
    TreeToExplore* treeToExplore,
    vector<TreeToExplore>& treesToExplore,
    LocalStore* db,
    git_repository* repo);
void serializeEntry(const git_tree_entry* entry, Appender* appender);
void gitCheckError(int error);
string getOID(const git_oid* oid);
}

namespace facebook {
namespace eden {

string doGitImport(const string& repoPath, const string& dbPath) {
  // The libgit2 library must be initialized before using it.
  git_libgit2_init();
  SCOPE_EXIT {
    git_libgit2_shutdown(); // Complement git_libgit2_init().
  };

  // Create a Git repository.
  git_repository* repo = nullptr;
  auto error = git_repository_open(&repo, repoPath.c_str());
  gitCheckError(error);
  SCOPE_EXIT {
    git_repository_free(repo);
  };

  // Create and then populate a RocksDB.
  auto db = std::make_shared<LocalStore>(dbPath);
  auto rootTreeHash = copyGitObjectsToDatabase(repo, db.get());

  // Huzzah!
  printf(
      "Success. Try running `ldb --db=%s scan`.\nRoot object is %s.\n",
      dbPath.c_str(),
      rootTreeHash.c_str());

  return rootTreeHash;
}
}
}

namespace {
const string copyGitObjectsToDatabase(git_repository* repo, LocalStore* db) {
  git_object* obj = nullptr;
  auto error = git_revparse_single(&obj, repo, "HEAD^{tree}");
  gitCheckError(error);

  auto treeObjectId = git_object_id(obj);
  auto rootTreeSha1 = getOID(treeObjectId);

  // Create a stack of TreeToExplore objects. When an instance gets to the top
  // of the stack:
  // * If TreeToExplore has an IOBuf, all of its children have already been
  //   written to the database, so write the tree's entry (which is stored in
  //   the IOBuf) to the database.
  // * If TreeToExplore does not have an IOBuf, iterate its children, writing
  //   child blobs to the database and populating the IOBuf.
  vector<TreeToExplore> treesToExplore;
  treesToExplore.emplace_back(reinterpret_cast<git_tree*>(obj));

  while (!treesToExplore.empty()) {
    TreeToExplore treeToExplore = std::move(treesToExplore.back());
    treesToExplore.pop_back();
    if (treeToExplore.buf) {
      SCOPE_EXIT {
        git_tree_free(treeToExplore.tree);
      };
      writeTreeEntryToDatabase(treeToExplore.tree, treeToExplore.buf.get(), db);
    } else {
      addChildrenToStack(&treeToExplore, treesToExplore, db, repo);
    }
  }

  return rootTreeSha1;
}

void writeTreeEntryToDatabase(git_tree* tree, IOBuf* buf, LocalStore* db) {
  // If this turns out to be a bottleneck, it may be possible to create an
  // adapter from an IOBuf to a Slice without using coalesce(). There's some
  // discussion on this in https://github.com/facebook/rocksdb/issues/958.
  buf->coalesce();

  // Add an entry to the DB for this tree object.
  auto treeOid = git_tree_id(tree);
  auto key = hashForOid(treeOid);
  db->putTree(key, ByteRange(buf->data(), buf->length()));
}

Hash hashForOid(const git_oid* oid) {
  DCHECK(Hash::RAW_SIZE == GIT_OID_RAWSZ);
  ByteRange bytes(
      reinterpret_cast<const unsigned char*>(oid->id), GIT_OID_RAWSZ);
  return Hash(bytes);
}

/**
 * Pushes the following objects onto the stack:
 * 1. A TreeToExplore for the specified treeToExplore with its IOBuf set. The
 *    IOBuf will be populated and is ready to be written to the database.
 * 2. A TreeToExplore with a null IOBuf for each of its children.
 *
 * This function also takes care of writing any child blobs to the database.
 */
void addChildrenToStack(
    TreeToExplore* treeToExplore,
    vector<TreeToExplore>& treesToExplore,
    LocalStore* db,
    git_repository* repo) {
  auto tree = treeToExplore->tree;
  size_t numEntries = git_tree_entrycount(tree);

  // Write the header for the Git tree object. This requires calculating the
  // length of the uncompressed contents.
  size_t contentSize = 0;
  for (int i = 0; i < numEntries; i++) {
    auto entry = git_tree_entry_byindex(tree, i);
    auto type = git_tree_entry_type(entry);
    auto lengthForType = type == GIT_OBJ_TREE
        ? TREE_MODE_OCTAL_LENGTH
        : BLOB_OR_COMMIT_MODE_OCTAL_LENGTH;
    auto name = git_tree_entry_name(entry);
    contentSize += lengthForType + 1 + strlen(name) + 1 + GIT_OID_RAWSZ;
  }

  // Include the length of the header to get the total size.
  string contentSizeStr = folly::to<string>(contentSize);
  size_t totalSize = 5 + contentSizeStr.length() + 1 + contentSize;

  auto newBuf = IOBuf::create(totalSize);
  IOBuf* buf = newBuf.get();
  treesToExplore.emplace_back(treeToExplore->tree, std::move(newBuf));
  // Although we do not expect to need to grow the buffer, specifying a `growth`
  // of 0 seems a bit aggressive.
  auto appender = Appender(buf, /* growth */ 10);
  appender("tree ");
  appender(contentSizeStr);
  appender.write<uint8_t>(0);

  // Iterate the entries in order (which in the case of Git, means
  // alphabetically, by name).
  for (int i = 0; i < numEntries; i++) {
    // Append a line for the current entry to the buffer.
    auto entry = git_tree_entry_byindex(tree, i);
    serializeEntry(entry, &appender);

    auto type = git_tree_entry_type(entry);
    if (type == GIT_OBJ_BLOB) {
      // Write an entry for the blob into the database.
      git_blob* blob = nullptr;
      auto oid = git_tree_entry_id(entry);
      int error = git_blob_lookup(&blob, repo, oid);
      gitCheckError(error);
      SCOPE_EXIT {
        git_blob_free(blob);
      };

      // Create the entry for the blob data.
      auto blobKey = hashForOid(oid);
      git_off_t rawsize = git_blob_rawsize(blob);
      const void* rawcontent = git_blob_rawcontent(blob);
      // TODO(mbolin): Figure out how to create this slice with a header for the
      // blob object: `blob ${rawsize}${NUL}`. We might need to update RocksDB
      // to take an IOBuf to do this without copying.
      auto buf = IOBuf::create(rawsize + 64 /*leave some room for the header*/);
      auto blobAppender =
          Appender(buf.get(), /*growth: should be unnecessary*/ 64);
      format("blob {}", rawsize)(blobAppender);
      blobAppender.write<uint8_t>(0);
      blobAppender.push(static_cast<const uint8_t*>(rawcontent), rawsize);

      // Create the entry for the SHA-1 of the blob's file contents whose key
      // can trivially be derived from the blob's key.
      unsigned char messageDigest[Hash::RAW_SIZE];
      SHA1(
          reinterpret_cast<const unsigned char*>(rawcontent),
          rawsize,
          messageDigest);

      db->putBlob(
          blobKey,
          ByteRange(buf->data(), buf->length()),
          Hash(ByteRange(messageDigest, Hash::RAW_SIZE)));
    } else if (type == GIT_OBJ_TREE) {
      // Add more work to the treesToExplore stack.
      git_object* subtree = nullptr;
      int error = git_object_lookup(
          &subtree, repo, git_tree_entry_id(entry), GIT_OBJ_TREE);
      gitCheckError(error);

      // TODO(mbolin): If the tree object already exists in the RocksDB, we
      // could have an option where we no longer recurse the tree object under
      // the expectation that the entire subtree has already been added?
      treesToExplore.emplace_back(reinterpret_cast<git_tree*>(subtree));
    }
  }

  CHECK_EQ(totalSize, buf->length())
      << "If contents do not match expected length, "
         "then the data may be corrupt.";
}

/**
 * Serializes the entry according to the following format, which matches that of
 * an entry in a Git tree object:
 *
 * - git_filemode_t, which determines permissions and file type. This is stored
 *   as an ASCII-encoded octal value (no leading zeroes).
 * - Space (0x20)
 * - name
 * - Nul (0x00)
 * - sha1 (20-byte hash)
 */
void serializeEntry(const git_tree_entry* entry, Appender* appender) {
  auto mode = git_tree_entry_filemode(entry);

  format("{:o}", static_cast<int>(mode))(*appender);
  appender->write<uint8_t>(0x20);

  auto name = git_tree_entry_name(entry);
  appender->push(StringPiece{name});
  appender->write<uint8_t>(0);

  auto oid = git_tree_entry_id(entry);
  appender->push(reinterpret_cast<const uint8_t*>(oid->id), GIT_OID_RAWSZ);
}

string getOID(const git_oid* oid) {
  char buf[GIT_OID_HEXSZ + 1];
  return git_oid_tostr(buf, GIT_OID_HEXSZ + 1, oid);
}

void gitCheckError(int error) {
  if (error) {
    auto lastError = giterr_last();
    auto errorMessage = stringPrintf(
        "Error %d/%d: %s\n", error, lastError->klass, lastError->message);
    throw std::runtime_error(errorMessage);
  }
}
}
