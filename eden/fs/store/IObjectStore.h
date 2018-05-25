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

#include <memory>
#include <vector>

namespace folly {
template <typename T>
class Future;
struct Unit;
}

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
