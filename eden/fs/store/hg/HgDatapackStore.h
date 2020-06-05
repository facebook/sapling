/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/futures/Promise.h>

#include "eden/fs/model/Blob.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/scm/lib/backingstore/c_api/HgNativeBackingStore.h"

namespace facebook {
namespace eden {

class Hash;
class HgProxyHash;

class HgDatapackStore {
 public:
  HgDatapackStore(AbsolutePathPiece repository, bool useEdenApi)
      : store_{repository.stringPiece(), useEdenApi} {}

  // Imports a blob for given hash from local store
  std::unique_ptr<Blob> getBlobLocal(const Hash& id, const HgProxyHash& hgInfo);

  // Imports a blob for given hash from remote store when it does not exist
  // locally.
  std::unique_ptr<Blob> getBlobRemote(
      const Hash& id,
      const HgProxyHash& hgInfo);

  /**
   * Import multiple blobs at once. The vector parameters have to be the same
   * length. Promises passed in will be resolved if a blob is successfully
   * imported. Otherwise the promise will be left untouched.
   */
  void getBlobBatch(
      const std::vector<Hash>& ids,
      const std::vector<HgProxyHash>& hashes,
      std::vector<folly::Promise<std::unique_ptr<Blob>>*> promises);

  std::unique_ptr<Tree> getTree(
      const RelativePath& path,
      const Hash& manifestId,
      const Hash& edenTreeId,
      LocalStore::WriteBatch* writeBatch);

  void refresh();

 private:
  HgNativeBackingStore store_;
};
} // namespace eden
} // namespace facebook
