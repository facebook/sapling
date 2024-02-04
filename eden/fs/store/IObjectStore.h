/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>
#include <vector>

#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace folly {
template <typename T>
class Future;
struct Unit;
} // namespace folly

namespace facebook::eden {

class Blob;
class BlobMetadata;
class ObjectId;
class Hash20;
class ObjectFetchContext;
template <typename T>
class ImmediateFuture;

class IObjectStore {
 public:
  virtual ~IObjectStore() = default;

  /*
   * Object access APIs.
   *
   * The given ObjectFetchContext must remain valid at least until the
   * resulting future is complete.
   */

  struct GetRootTreeResult {
    GetRootTreeResult(TreePtr tree, ObjectId treeId)
        : tree{std::move(tree)}, treeId{std::move(treeId)} {}

    GetRootTreeResult(const GetRootTreeResult&) = default;
    GetRootTreeResult(GetRootTreeResult&&) = default;

    GetRootTreeResult& operator=(const GetRootTreeResult&) = default;
    GetRootTreeResult& operator=(GetRootTreeResult&&) = default;

    /// The root tree object.
    TreePtr tree;
    /// The root tree's ID which can later be passed to getTree.
    ObjectId treeId;
  };

  virtual ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& rootId,
      const ObjectFetchContextPtr& context) const = 0;
  virtual ImmediateFuture<std::shared_ptr<const Tree>> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const = 0;
  virtual ImmediateFuture<std::shared_ptr<const Blob>> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context) const = 0;

  /**
   * Prefetch all the blobs represented by the HashRange.
   *
   * The caller is responsible for making sure that the HashRange stays valid
   * for as long as the returned ImmediateFuture.
   */
  virtual ImmediateFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      const ObjectFetchContextPtr& context) const = 0;
};

} // namespace facebook::eden
