/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ScmStatusCache.h"

namespace facebook::eden {

ScmStatusCache::ScmStatusCache(const EdenConfig* configPtr, EdenStatsPtr stats)
    : ObjectCache<
          SeqStatusPair,
          ObjectCacheFlavor::Simple,
          ScmStatusCacheStats>{
          configPtr->scmStatusCacheMaxSize.getValue(),
          configPtr->scmStatusCacheMininumItems.getValue(),
          std::move(stats)} {}

std::shared_ptr<const SeqStatusPair> ScmStatusCache::get(const ObjectId& hash) {
  return getSimple(hash);
}

void ScmStatusCache::insert(
    ObjectId id,
    std::shared_ptr<const SeqStatusPair> status) {
  auto existingStatus = get(id);
  if (!existingStatus) {
    insertSimple(std::move(id), std::move(status));
    return;
  }

  // it's only necessary to update the cache if the diff is computed
  // for a larger sequenceID than the existing one.
  if (status->seq > existingStatus->seq) {
    invalidate(id);
    insertSimple(std::move(id), std::move(status));
  }
}

ObjectId ScmStatusCache::makeKey(const RootId& commitHash, bool listIgnored) {
  return ObjectId(
      folly::fbstring(fmt::format("{}:{}", commitHash.value(), listIgnored)));
}

} // namespace facebook::eden
