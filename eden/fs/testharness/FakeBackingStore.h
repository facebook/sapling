/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <initializer_list>
#include <memory>
#include <unordered_map>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/testharness/StoredObject.h"

namespace facebook::eden {

class FakeTreeBuilder;

/**
 * A BackingStore implementation for test code.
 */
class FakeBackingStore final : public BackingStore {
 public:
  struct TreeEntryData;

  FakeBackingStore();
  ~FakeBackingStore() override;

  /*
   * BackingStore APIs
   */

  ObjectComparison compareObjectsById(const ObjectId& one, const ObjectId& two)
      override {
    // FakeBackingStore does not provide any particular ID scheme requirements.
    // If IDs match, contents must, but not the other way around.
    if (one.bytesEqual(two)) {
      return ObjectComparison::Identical;
    }
    return ObjectComparison::Unknown;
  }

  RootId parseRootId(folly::StringPiece rootId) override;
  std::string renderRootId(const RootId& rootId) override;
  ObjectId parseObjectId(folly::StringPiece objectId) override;
  std::string renderObjectId(const ObjectId& objectId) override;

  folly::SemiFuture<std::unique_ptr<Tree>> getRootTree(
      const RootId& commitID,
      ObjectFetchContext& context) override;
  folly::SemiFuture<std::unique_ptr<TreeEntry>> getTreeEntryForRootId(
      const RootId& /* commitID */,
      TreeEntryType /* treeEntryType */,
      ObjectFetchContext& /* context */) override;

  folly::SemiFuture<BackingStore::GetTreeRes> getTree(
      const ObjectId& id,
      ObjectFetchContext& context) override;
  folly::SemiFuture<BackingStore::GetBlobRes> getBlob(
      const ObjectId& id,
      ObjectFetchContext& context) override;

  /**
   * Add a Blob to the backing store
   *
   * If a hash is not explicitly given, one will be computed automatically.
   * (The test code may not use the same hashing scheme as a production
   * mercurial- or git-backed store, but it will be consistent for the
   * duration of the test.)
   */
  StoredBlob* putBlob(folly::StringPiece contents);
  StoredBlob* putBlob(ObjectId hash, folly::StringPiece contents);

  /**
   * Add a blob to the backing store, or return the StoredBlob already present
   * with this hash.
   *
   * The boolean in the return value is true if a new StoredBlob was created by
   * this call, or false if a StoredBlob already existed with this hash.
   */
  std::pair<StoredBlob*, bool> maybePutBlob(folly::StringPiece contents);
  std::pair<StoredBlob*, bool> maybePutBlob(
      ObjectId hash,
      folly::StringPiece contents);

  static Blob makeBlob(folly::StringPiece contents);
  static Blob makeBlob(ObjectId hash, folly::StringPiece contents);

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
      ObjectId hash,
      const std::initializer_list<TreeEntryData>& entries);
  StoredTree* putTree(Tree::container entries);
  StoredTree* putTree(ObjectId hash, Tree::container entries);

  /**
   * Add a tree to the backing store, or return the StoredTree already present
   * with this hash.
   *
   * The boolean in the return value is true if a new StoredTree was created by
   * this call, or false if a StoredTree already existed with this hash.
   */
  std::pair<StoredTree*, bool> maybePutTree(
      const std::initializer_list<TreeEntryData>& entries);
  std::pair<StoredTree*, bool> maybePutTree(Tree::container entries);

  /**
   * Add a mapping from a commit ID to a root tree hash.
   */
  StoredHash* putCommit(const RootId& commitHash, const StoredTree* tree);
  StoredHash* putCommit(const RootId& commitHash, ObjectId treeHash);
  StoredHash* putCommit(
      const RootId& commitHash,
      const FakeTreeBuilder& builder);
  StoredHash* putCommit(
      folly::StringPiece commitStr,
      const FakeTreeBuilder& builder);

  /**
   * Look up a StoredTree.
   *
   * Throws an error if the specified hash does not exist.  Never returns null.
   */
  StoredTree* getStoredTree(ObjectId hash);

  /**
   * Look up a StoredBlob.
   *
   * Throws an error if the specified hash does not exist.  Never returns null.
   */
  StoredBlob* getStoredBlob(ObjectId hash);

  /**
   * Manually clear the list of outstanding requests to avoid cycles during
   * TestMount destruction.
   */
  void discardOutstandingRequests();

  /**
   * Returns the number of times this hash has been queried by either getTree,
   * getBlob, or getTreeForCommit.
   */
  size_t getAccessCount(const ObjectId& hash) const;

  // TODO(T119221752): Implement for all BackingStore subclasses
  int64_t dropAllPendingRequestsFromQueue() override {
    XLOG(
        WARN,
        "dropAllPendingRequestsFromQueue() is not implemented for FakeBackingStore");
    return 0;
  }

 private:
  struct Data {
    std::unordered_map<RootId, std::unique_ptr<StoredHash>> commits;
    std::unordered_map<ObjectId, std::unique_ptr<StoredTree>> trees;
    std::unordered_map<ObjectId, std::unique_ptr<StoredBlob>> blobs;

    std::unordered_map<RootId, size_t> commitAccessCounts;
    std::unordered_map<ObjectId, size_t> accessCounts;
  };

  static Tree::container buildTreeEntries(
      const std::initializer_list<TreeEntryData>& entryArgs);
  static ObjectId computeTreeHash(const Tree::container& sortedEntries);
  StoredTree* putTreeImpl(ObjectId hash, Tree::container&& sortedEntries);
  std::pair<StoredTree*, bool> maybePutTreeImpl(
      ObjectId hash,
      Tree::container&& sortedEntries);

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

  Tree::value_type entry;
};
} // namespace facebook::eden
