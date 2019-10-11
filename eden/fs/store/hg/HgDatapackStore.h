/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/Synchronized.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/utils/PathFuncs.h"

#include "scm/hg/lib/revisionstore/RevisionStore.h" // @manual

namespace facebook {
namespace eden {

class ReloadableConfig;
class Hash;
class HgProxyHash;

class HgDatapackStore {
 public:
  HgDatapackStore(
      AbsolutePathPiece repository,
      folly::StringPiece repoName,
      AbsolutePathPiece cachePath,
      RelativePathPiece subdir);

  std::unique_ptr<Blob> getBlob(const Hash& id, const HgProxyHash& hgInfo);

 private:
  std::optional<folly::Synchronized<DataPackUnion>> store_;
};

std::optional<HgDatapackStore> makeHgDatapackStore(
    AbsolutePathPiece repository,
    std::shared_ptr<ReloadableConfig> edenConfig);
} // namespace eden
} // namespace facebook
