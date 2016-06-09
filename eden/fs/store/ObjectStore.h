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

#include <memory>

namespace facebook {
namespace eden {

class Blob;
class Hash;
class LocalStore;
class Tree;

/**
 * ObjectStore is a content-addressed store for eden object data.
 *
 * The ObjectStore class itself is primarily a wrapper around two other
 * underlying storage types:
 * - LocalStore, which caches object data locally in a RocksDB instance
 * - BackingStore, which represents the authoritative source for the object
 *   data.  The BackingStore is generally more expensive to query for object
 *   data, and may not be available during offline operation.
 */
class ObjectStore {
 public:
  explicit ObjectStore(std::shared_ptr<LocalStore> localStore);
  virtual ~ObjectStore();

  std::unique_ptr<Tree> getTree(const Hash& id) const;
  std::unique_ptr<Blob> getBlob(const Hash& id) const;

  /**
   * Return the SHA1 hash of the blob contents.
   *
   * (Note that this is different than the Hash identifying the blob.  The
   * hash identifying the blob may be computed using a separate mechanism, and
   * may not be the same as the SHA1-hash of its contents.)
   */
  std::unique_ptr<Hash> getSha1ForBlob(const Hash& id) const;

 private:
  // Forbidden copy constructor and assignment operator
  ObjectStore(ObjectStore const&) = delete;
  ObjectStore& operator=(ObjectStore const&) = delete;

  /*
   * The LocalStore.
   *
   * Multiple ObjectStores (for different mount points) may share the same
   * LocalStore.
   */
  std::shared_ptr<LocalStore> localStore_;
};
}
} // facebook::eden
