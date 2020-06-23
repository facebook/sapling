/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>
#include <vector>

#include <folly/portability/SysTypes.h>
#include "eden/fs/store/ImportPriority.h"

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
class ObjectFetchContext;

class IObjectStore {
 public:
  virtual ~IObjectStore() {}

  /*
   * Object access APIs.
   *
   * The given ObjectFetchContext must remain valid at least until the
   * resulting future is complete.
   */

  virtual folly::Future<std::shared_ptr<const Tree>> getTree(
      const Hash& id,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<std::shared_ptr<const Blob>> getBlob(
      const Hash& id,
      ObjectFetchContext& context,
      ImportPriority priority = ImportPriority::kNormal()) const = 0;
  virtual folly::Future<std::shared_ptr<const Tree>> getTreeForCommit(
      const Hash& commitID,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<std::shared_ptr<const Tree>> getTreeForManifest(
      const Hash& commitID,
      const Hash& manifestID,
      ObjectFetchContext& context) const = 0;
  virtual folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids,
      ObjectFetchContext& context) const = 0;
};
} // namespace eden
} // namespace facebook
