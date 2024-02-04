/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/BlobAccess.h"
#include <folly/MapUtil.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/IObjectStore.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

BlobAccess::BlobAccess(
    std::shared_ptr<IObjectStore> objectStore,
    std::shared_ptr<BlobCache> blobCache)
    : objectStore_{std::move(objectStore)}, blobCache_{std::move(blobCache)} {}

BlobAccess::~BlobAccess() = default;

ImmediateFuture<BlobCache::GetResult> BlobAccess::getBlob(
    const ObjectId& hash,
    const ObjectFetchContextPtr& context,
    BlobCache::Interest interest) {
  auto result = blobCache_->get(hash, interest);
  if (result.object) {
    return folly::Future<BlobCache::GetResult>{std::move(result)};
  }

  return objectStore_->getBlob(hash, context)
      .thenValue([blobCache = blobCache_, hash = hash, interest](
                     std::shared_ptr<const Blob> blob) mutable {
        auto interestHandle =
            blobCache->insert(std::move(hash), blob, interest);
        return BlobCache::GetResult{std::move(blob), std::move(interestHandle)};
      });
}

} // namespace facebook::eden
