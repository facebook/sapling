/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <gtest/gtest_prod.h>
#include <initializer_list>
#include <memory>
#include <string>
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
class ServerState;

/**
 * A BackingStore implementation for test code.
 */
class FakeBackingStore final : public BackingStore {
 public:
  struct TreeEntryData;

  explicit FakeBackingStore(
      LocalStoreCachingPolicy localStoreCachingPolicy,
      std::shared_ptr<ServerState> serverState = nullptr,
      std::optional<std::string> blake3Key = std::nullopt);
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

  LocalStoreCachingPolicy getLocalStoreCachingPolicy() const override {
    return localStoreCachingPolicy_;
  }

  /**
   * Add a Blob to the backing store
   *
   * If an id is not explicitly given, one will be computed automatically.
   * (The test code may not use the same id scheme as a production
   * mercurial- or git-backed store, but it will be consistent for the
   * duration of the test.)
   */
  std::pair<StoredBlob*, ObjectId> putBlob(folly::StringPiece contents);
  StoredBlob* putBlob(ObjectId id, folly::StringPiece contents);

  /**
   * Add a blob to the backing store, or return the StoredBlob already present
   * with this id.
   *
   * The boolean in the return value is true if a new StoredBlob was created by
   * this call, or false if a StoredBlob already existed with this id.
   */
  std::tuple<StoredBlob*, ObjectId, bool> maybePutBlob(
      folly::StringPiece contents);
  std::tuple<StoredBlob*, ObjectId, bool> maybePutBlob(
      ObjectId id,
      folly::StringPiece contents);

  static Blob makeBlob(folly::StringPiece contents);

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
      ObjectId id,
      const std::initializer_list<TreeEntryData>& entries);
  StoredTree* putTree(Tree::container entries);
  StoredTree* putTree(ObjectId id, Tree::container entries);

  /**
   * Add a tree to the backing store, or return the StoredTree already present
   * with this id.
   *
   * The boolean in the return value is true if a new StoredTree was created by
   * this call, or false if a StoredTree already existed with this id.
   */
  std::pair<StoredTree*, bool> maybePutTree(
      const std::initializer_list<TreeEntryData>& entries);
  std::pair<StoredTree*, bool> maybePutTree(Tree::container entries);

  /**
   * Add a mapping from a commit ID to a root tree id.
   */
  StoredId* putCommit(const RootId& commitId, const StoredTree* tree);
  StoredId* putCommit(const RootId& commitId, ObjectId treeId);
  StoredId* putCommit(const RootId& commitId, const FakeTreeBuilder& builder);
  StoredId* putCommit(
      folly::StringPiece commitStr,
      const FakeTreeBuilder& builder);

  /**
   * Add a Glob to the backing store
   */
  StoredGlob* putGlob(
      std::pair<RootId, std::string> suffixQuery,
      std::vector<std::string> contents);

  /**
   * Look up a StoredTree.
   *
   * Throws an error if the specified id does not exist.  Never returns null.
   */
  StoredTree* getStoredTree(ObjectId id);

  /**
   * Look up a StoredBlob.
   *
   * Throws an error if the specified id does not exist.  Never returns null.
   */
  StoredBlob* getStoredBlob(ObjectId id);

  /**
   * Look up a StoredGlob.
   *
   * Throws an error if the specified id does not exist.  Never returns null.
   */
  StoredGlob* getStoredGlob(std::pair<RootId, std::string> suffixQuery);

  /**
   * Manually clear the list of outstanding requests to avoid cycles during
   * TestMount destruction.
   */
  void discardOutstandingRequests();

  /**
   * Returns the number of times this id has been queried by either getTree,
   * getBlob, or getTreeForCommit.
   */
  size_t getAccessCount(const ObjectId& id) const;

  // TODO(T119221752): Implement for all BackingStore subclasses
  int64_t dropAllPendingRequestsFromQueue() override {
    XLOG(
        WARN,
        "dropAllPendingRequestsFromQueue() is not implemented for FakeBackingStore");
    return 0;
  }

  std::vector<ObjectId> getAuxDataLookups() const {
    return data_.rlock()->auxDataLookups;
  }

 private:
  struct Data {
    std::unordered_map<RootId, std::unique_ptr<StoredId>> commits;
    std::unordered_map<ObjectId, std::unique_ptr<StoredTree>> trees;
    std::unordered_map<ObjectId, std::unique_ptr<StoredBlob>> blobs;
    std::unordered_map<
        std::pair<RootId, std::string>,
        std::unique_ptr<StoredGlob>>
        globs;

    std::unordered_map<RootId, size_t> commitAccessCounts;
    std::unordered_map<ObjectId, size_t> accessCounts;
    std::vector<ObjectId> auxDataLookups;
  };

  static Tree::container buildTreeEntries(
      const std::initializer_list<TreeEntryData>& entryArgs);
  static ObjectId computeTreeId(const Tree::container& sortedEntries);
  StoredTree* putTreeImpl(ObjectId id, Tree::container&& sortedEntries);
  std::pair<StoredTree*, bool> maybePutTreeImpl(
      ObjectId id,
      Tree::container&& sortedEntries);

  FRIEND_TEST(FakeBackingStoreTest, getNonExistent);
  FRIEND_TEST(FakeBackingStoreTest, getBlob);
  FRIEND_TEST(FakeBackingStoreTest, getTree);
  FRIEND_TEST(FakeBackingStoreTest, getRootTree);
  FRIEND_TEST(FakeBackingStoreTest, getGlobFiles);

  ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& commitID,
      const ObjectFetchContextPtr& context) override;
  ImmediateFuture<std::shared_ptr<TreeEntry>> getTreeEntryForObjectId(
      const ObjectId& /* commitID */,
      TreeEntryType /* treeEntryType */,
      const ObjectFetchContextPtr& /* context */) override;

  folly::SemiFuture<GetTreeResult> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<GetTreeAuxResult> getTreeAuxData(
      const ObjectId& /*id*/,
      const ObjectFetchContextPtr& /*context*/) override;
  folly::SemiFuture<GetBlobResult> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  folly::SemiFuture<GetBlobAuxResult> getBlobAuxData(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) override;
  ImmediateFuture<GetGlobFilesResult> getGlobFiles(
      const RootId& id,
      const std::vector<std::string>& globs,
      const std::vector<std::string>& prefixes) override;

  LocalStoreCachingPolicy localStoreCachingPolicy_;
  std::shared_ptr<ServerState> serverState_;
  folly::Synchronized<Data> data_;
  std::optional<std::string> blake3Key_;
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
  // blob
  TreeEntryData(
      folly::StringPiece name,
      const ObjectId& id,
      FakeBlobType type = FakeBlobType::REGULAR_FILE);
  // blob
  TreeEntryData(
      folly::StringPiece name,
      const std::pair<StoredBlob*, ObjectId>& blob,
      FakeBlobType type = FakeBlobType::REGULAR_FILE);
  // tree
  TreeEntryData(folly::StringPiece name, const Tree& tree);
  // tree
  TreeEntryData(folly::StringPiece name, const StoredTree* tree);

  Tree::value_type entry;
};
} // namespace facebook::eden
