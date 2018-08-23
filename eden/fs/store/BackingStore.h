/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/futures/Future.h>
#include <memory>

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class Blob;
class Hash;
class Tree;

/**
 * Abstract interface for a BackingStore.
 *
 * A BackingStore fetches tree and blob information from an external
 * authoritative data source.
 *
 * BackingStore implementations must be thread-safe, and perform their own
 * internal locking.
 */
class BackingStore {
 public:
  BackingStore() {}
  virtual ~BackingStore() {}

  virtual folly::Future<std::unique_ptr<Tree>> getTree(const Hash& id) = 0;
  virtual folly::Future<std::unique_ptr<Blob>> getBlob(const Hash& id) = 0;
  virtual folly::Future<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) = 0;
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) const {
    return folly::unit;
  }

  /**
   * Attempt to re-verify the contents of a previously imported blob that was
   * recorded as empty.  This is unfortunately necessary at the moment since
   * we have seen bugs where some files were incorrectly recorded as empty in
   * the LocalStore.
   *
   * This returns a null pointer if the Blob has been verified as empty, or a
   * new Blob if the file is not empty.
   */
  virtual folly::Future<std::unique_ptr<Blob>> verifyEmptyBlob(const Hash& id);

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;
};
} // namespace eden
} // namespace facebook
