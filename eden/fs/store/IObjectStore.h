/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <vector>

namespace folly {
template <typename T>
class Future;
struct Unit;
} // namespace folly

namespace facebook {
namespace eden {

class Blob;
class BlobMetadata;
class Hash;
class Tree;

class IObjectStore {
 public:
  virtual ~IObjectStore() {}

  /*
   * Object access APIs.
   */
  virtual folly::Future<std::shared_ptr<const Tree>> getTree(
      const Hash& id) const = 0;
  virtual folly::Future<std::shared_ptr<const Blob>> getBlob(
      const Hash& id) const = 0;
  virtual folly::Future<std::shared_ptr<const Tree>> getTreeForCommit(
      const Hash& commitID) const = 0;
  virtual folly::Future<BlobMetadata> getBlobMetadata(const Hash& id) const = 0;
  virtual folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) const = 0;
};
} // namespace eden
} // namespace facebook
