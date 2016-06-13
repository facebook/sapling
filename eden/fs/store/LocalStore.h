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

#include <folly/Range.h>
#include <memory>

namespace rocksdb {
class DB;
}

namespace facebook {
namespace eden {

class Blob;
class Hash;
class StoreResult;
class Tree;

/*
 * LocalStore stores objects (trees and blobs) locally on disk.
 *
 * This is a content-addressed store, so objects can be only retrieved using
 * their hash.
 *
 * The LocalStore is only a cache.  If an object is not found in the LocalStore
 * then it will need to be retrieved from the BackingStore.
 *
 * LocalStore uses RocksDB for the underlying storage.
 *
 * LocalStore is thread-safe, and can be used from multiple threads without
 * requiring the caller to perform locking around accesses to the LocalStore.
 */
class LocalStore {
 public:
  explicit LocalStore(folly::StringPiece pathToRocksDb);
  virtual ~LocalStore();

  /**
   * Get arbitrary unserialized data from the store.
   *
   * StoreResult::isValid() will be true if the key was found, and false
   * if the key was not present.
   *
   * May throw exceptions on error.
   */
  StoreResult get(folly::ByteRange key) const;
  StoreResult get(const Hash& id) const;

  /**
   * Get a Tree from the store.
   *
   * Returns nullptr if this key is not present in the store.
   * May throw exceptions on error (e.g., if this ID refers to a non-tree
   * object).
   */
  std::unique_ptr<Tree> getTree(const Hash& id) const;

  /**
   * Get a Blob from the store.
   *
   * Blob objects store file data.
   *
   * Returns nullptr if this key is not present in the store.
   * May throw exceptions on error (e.g., if this ID refers to a non-blob
   * object).
   */
  std::unique_ptr<Blob> getBlob(const Hash& id) const;

  /**
   * Get the SHA-1 hash of the blob contents for the specified blob.
   *
   * Returns nullptr if this key is not present in the store, or throws an
   * exception on error.
   */
  std::unique_ptr<Hash> getSha1ForBlob(const Hash& id) const;

  void putTree(const Hash& id, folly::ByteRange treeData);
  void putBlob(const Hash& id, folly::ByteRange blobData, const Hash& sha1);

  /**
   * Put arbitrary data in the store.
   */
  void put(folly::ByteRange key, folly::ByteRange value);

 private:
  std::unique_ptr<rocksdb::DB> db_;
};
}
}
