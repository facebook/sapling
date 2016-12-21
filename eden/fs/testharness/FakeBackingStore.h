/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
#include "eden/fs/testharness/StoredObject.h"

namespace facebook {
namespace eden {

class LocalStore;

/**
 * A BackingStore implementation for test code.
 */
class FakeBackingStore : public BackingStore {
 public:
  struct TreeEntryData;

  explicit FakeBackingStore(std::shared_ptr<LocalStore> localStore);
  virtual ~FakeBackingStore();

  /*
   * BackingStore APIs
   */

  folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) override;
  folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) override;
  folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) override;

  /**
   * Add a Blob to the Backing store
   */
  StoredBlob* putBlob(folly::StringPiece contents);
  StoredBlob* putBlob(Hash hash, folly::StringPiece contents);

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

  /**
   * Add a mapping from a commit ID to a root tree hash.
   */
  StoredHash* putCommit(Hash commitHash, const StoredTree* tree);
  StoredHash* putCommit(Hash commitHash, Hash treeHash);

 private:
  struct Data {
    std::unordered_map<Hash, std::unique_ptr<StoredTree>> trees;
    std::unordered_map<Hash, std::unique_ptr<StoredBlob>> blobs;
    std::unordered_map<Hash, std::unique_ptr<StoredHash>> commits;
  };

  StoredTree* putTreeImpl(Hash hash, std::vector<TreeEntry>&& entries);

  const std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<Data> data_;
};

/**
 * A small helper struct for use with FakeBackingStore::putTree()
 *
 * This mainly exists to allow putTree() to be called conveniently with
 * initialier-list arguments.
 */
struct FakeBackingStore::TreeEntryData {
  TreeEntryData(folly::StringPiece name, const Blob& blob, mode_t mode = 0644);
  TreeEntryData(
      folly::StringPiece name,
      const StoredBlob* blob,
      mode_t mode = 0644);
  TreeEntryData(folly::StringPiece name, const Tree& tree, mode_t mode = 0755);
  TreeEntryData(
      folly::StringPiece name,
      const StoredTree* tree,
      mode_t mode = 0755);

  TreeEntry entry;
};
}
} // facebook::eden
