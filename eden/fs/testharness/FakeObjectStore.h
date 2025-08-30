/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <unordered_map>

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

/**
 * Fake implementation of IObjectStore that allows the data to be injected
 * directly. This is designed to be used for unit tests.
 */
class FakeObjectStore final : public IObjectStore {
 public:
  FakeObjectStore();
  ~FakeObjectStore() override;

  void addTree(Tree&& tree);
  void addBlob(ObjectId id, Blob&& blob);
  void setTreeForCommit(const RootId& commitID, Tree&& tree);

  ImmediateFuture<GetRootTreeResult> getRootTree(
      const RootId& commitID,
      const ObjectFetchContextPtr& context =
          ObjectFetchContext::getNullContext()) const override;
  ImmediateFuture<std::shared_ptr<const Tree>> getTree(
      const ObjectId& id,
      const ObjectFetchContextPtr& context =
          ObjectFetchContext::getNullContext()) const override;
  ImmediateFuture<std::shared_ptr<const Blob>> getBlob(
      const ObjectId& id,
      const ObjectFetchContextPtr& context =
          ObjectFetchContext::getNullContext()) const override;
  ImmediateFuture<folly::Unit> prefetchBlobs(
      ObjectIdRange ids,
      const ObjectFetchContextPtr& context =
          ObjectFetchContext::getNullContext()) const override;

  size_t getAccessCount(const ObjectId& id) const;

 private:
  std::unordered_map<RootId, Tree> commits_;
  std::unordered_map<ObjectId, Tree> trees_;
  std::unordered_map<ObjectId, Blob> blobs_;
  mutable std::unordered_map<RootId, size_t> commitAccessCounts_;
  mutable std::unordered_map<ObjectId, size_t> accessCounts_;
};
} // namespace facebook::eden
