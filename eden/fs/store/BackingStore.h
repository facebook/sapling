/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
  virtual folly::SemiFuture<std::unique_ptr<Blob>> getBlob(const Hash& id) = 0;
  virtual folly::SemiFuture<std::unique_ptr<Tree>> getTreeForCommit(
      const Hash& commitID) = 0;
  virtual folly::SemiFuture<std::unique_ptr<Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID) = 0;
  FOLLY_NODISCARD virtual folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& /*ids*/) const {
    return folly::unit;
  }

 private:
  // Forbidden copy constructor and assignment operator
  BackingStore(BackingStore const&) = delete;
  BackingStore& operator=(BackingStore const&) = delete;
};
} // namespace eden
} // namespace facebook
