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

#include <unordered_map>
#include "eden/fs/store/IObjectStore.h"

#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/BlobMetadata.h"

namespace facebook {
namespace eden {

/**
 * Fake implementation of IObjectStore that allows the data to be injected
 * directly. This is designed to be used for unit tests.
 */
class FakeObjectStore : public IObjectStore {
 public:
  FakeObjectStore();
  ~FakeObjectStore() override;

  void addTree(Tree&& tree);
  void addBlob(Blob&& blob);
  void setTreeForCommit(const Hash& commitID, Tree&& tree);

  folly::Future<std::shared_ptr<const Tree>> getTree(
      const Hash& id) const override;
  folly::Future<std::shared_ptr<const Blob>> getBlob(
      const Hash& id) const override;
  folly::Future<std::shared_ptr<const Tree>> getTreeForCommit(
      const Hash& commitID) const override;
  folly::Future<BlobMetadata> getBlobMetadata(const Hash& id) const override;
  folly::Future<folly::Unit> prefetchBlobs(
      const std::vector<Hash>& ids) const override;

 private:
  std::unordered_map<Hash, Tree> trees_;
  std::unordered_map<Hash, Blob> blobs_;
  std::unordered_map<Hash, Tree> commits_;
  std::unordered_map<Hash, BlobMetadata> blobMetadata_;
};
} // namespace eden
} // namespace facebook
