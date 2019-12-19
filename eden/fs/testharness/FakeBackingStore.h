/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <initializer_list>
#include <memory>
#include <unordered_map>
#include <vector>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/testharness/StoredObject.h"

namespace facebook {
namespace eden {

class FakeTreeBuilder;
class LocalStore;

/**
 * A BackingStore implementation for test code.
 */
class FakeBackingStore : public BackingStore {
 public:
  struct TreeEntryData;

  explicit FakeBackingStore(std::shared_ptr<LocalStore> localStore);
  ~FakeBackingStore() override;

  /*
   * BackingStore APIs
   */

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::SemiFuture<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;
  folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) override;
  /**
   * Add a Blob to the backing store
   *
   * If a hash is not explicitly given, one will be computed automatically.
   * (The test code may not use the same hashing scheme as a production
   * mercurial- or git-backed store, but it will be consistent for the
   * duration of the test.)
   */
  StoredBlob* putBlob(folly::StringPiece contents);
  StoredBlob* putBlob(Hash hash, folly::StringPiece contents);

  /**
   * Add a blob to the backing store, or return the StoredBlob already present
   * with this hash.
   *
   * The boolean in the return value is true if a new StoredBlob was created by
   * this call, or false if a StoredBlob already existed with this hash.
   */
  std::pair<StoredBlob*, bool> maybePutBlob(folly::StringPiece contents);
  std::pair<StoredBlob*, bool> maybePutBlob(
      Hash hash,
      folly::StringPiece contents);

  static Blob makeBlob(folly::StringPiece contents);
  static Blob makeBlob(Hash hash, folly::StringPiece contents);

  /**
   * Helper functions for building a tree.
   *
   * Example usage:
   *
   *   store->putTree({
   *       {"test.txt", testBlob, 0644},
   *       {"runme.sh", runmeBlob, 0755},
   *       {"subdir", subdirTree, 0755},
   *   });
   */
  StoredTree* putTree(const std::initializer_list<TreeEntryData>& entries);
  StoredTree* putTree(
      Hash hash,
      const std::initializer_list<TreeEntryData>& entries);
  StoredTree* putTree(std::vector<TreeEntry> entries);
  StoredTree* putTree(Hash hash, std::vector<TreeEntry> entries);

  /**
   * Add a tree to the backing store, or return the StoredTree already present
   * with this hash.
   *
   * The boolean in the return value is true if a new StoredTree was created by
   * this call, or false if a StoredTree already existed with this hash.
   */
  std::pair<StoredTree*, bool> maybePutTree(
      const std::initializer_list<TreeEntryData>& entries);
  std::pair<StoredTree*, bool> maybePutTree(std::vector<TreeEntry> entries);

  /**
   * Add a mapping from a commit ID to a root tree hash.
   */
  StoredHash* putCommit(Hash commitHash, const StoredTree* tree);
  StoredHash* putCommit(Hash commitHash, Hash treeHash);
  StoredHash* putCommit(Hash commitHash, const FakeTreeBuilder& builder);
  StoredHash* putCommit(
      folly::StringPiece commitStr,
      const FakeTreeBuilder& builder);

  /**
   * Look up a StoredTree.
   *
   * Throws an error if the specified hash does not exist.  Never returns null.
   */
  StoredTree* getStoredTree(Hash hash);

  /**
   * Look up a StoredBlob.
   *
   * Throws an error if the specified hash does not exist.  Never returns null.
   */
  StoredBlob* getStoredBlob(Hash hash);

  /**
   * Manually clear the list of outstanding requests to avoid cycles during
   * TestMount destruction.
   */
  void discardOutstandingRequests();

  /**
   * Returns the number of times this hash has been queried by either getTree,
   * getBlob, or getTreeForCommit.
   */
  size_t getAccessCount(const Hash& hash) const;

 private:
  struct Data {
    std::unordered_map<Hash, std::unique_ptr<StoredTree>> trees;
    std::unordered_map<Hash, std::unique_ptr<StoredBlob>> blobs;
    std::unordered_map<Hash, std::unique_ptr<StoredHash>> commits;
    std::unordered_map<Hash, size_t> accessCounts;
  };

  static std::vector<TreeEntry> buildTreeEntries(
      const std::initializer_list<TreeEntryData>& entryArgs);
  static void sortTreeEntries(std::vector<TreeEntry>& entries);
  static Hash computeTreeHash(const std::vector<TreeEntry>& sortedEntries);
  StoredTree* putTreeImpl(Hash hash, std::vector<TreeEntry>&& sortedEntries);
  std::pair<StoredTree*, bool> maybePutTreeImpl(
      Hash hash,
      std::vector<TreeEntry>&& sortedEntries);

  const std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<Data> data_;
};

enum class FakeBlobType {
  REGULAR_FILE,
  EXECUTABLE_FILE,
  SYMLINK,
};

/**
 * A small helper struct for use with FakeBackingStore::putTree()
 *
 * This mainly exists to allow putTree() to be called conveniently with
 * initialier-list arguments.
 */
struct FakeBackingStore::TreeEntryData {
  TreeEntryData(
      folly::StringPiece name,
      const Blob& blob,
      FakeBlobType type = FakeBlobType::REGULAR_FILE);
  TreeEntryData(
      folly::StringPiece name,
      const StoredBlob* blob,
      FakeBlobType type = FakeBlobType::REGULAR_FILE);
  // tree
  TreeEntryData(folly::StringPiece name, const Tree& tree);
  // tree
  TreeEntryData(folly::StringPiece name, const StoredTree* tree);

  TreeEntry entry;
};
} // namespace eden
} // namespace facebook
