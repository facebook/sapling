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

std::variant<StatusResultFuture, StatusResultPromise> ScmStatusCache::get(
    const ObjectId& id,
    JournalDelta::SequenceNumber seq) {
  auto internalCachedItem = getSimple(id);
  if (internalCachedItem && internalCachedItem->seq >= seq) {
    return ImmediateFuture<ScmStatus>{internalCachedItem->status};
  }

  auto it = promiseMap_.find(id);
  if (it != promiseMap_.end() && it->second.first >= seq) {
    return it->second.second->getFuture();
  }

  auto promise = std::make_shared<folly::SharedPromise<ScmStatus>>();
  promiseMap_.insert_or_assign(id, std::make_pair(seq, promise));

  return promise;
}

void ScmStatusCache::insert(
    ObjectId id,
    std::shared_ptr<const SeqStatusPair> pair) {
  auto internalCachedItem = getSimple(id);

  if (!internalCachedItem) {
    insertSimple(std::move(id), pair);
    return;
  }

  // it's only necessary to update the cache if the diff is computed
  // for a larger sequenceID than the existing one.
  if (pair->seq > internalCachedItem->seq) {
    invalidate(id);
    insertSimple(std::move(id), std::move(pair));
  }
}

void ScmStatusCache::dropPromise(
    const ObjectId& key,
    JournalDelta::SequenceNumber seq) {
  auto it = promiseMap_.find(key);
  // we don't want to accidentally drop promises owned by other requests
  // which query with a larger sequence number
  if (it != promiseMap_.end() && it->second.first == seq) {
    promiseMap_.erase(key);
  }
}

ObjectId ScmStatusCache::makeKey(const RootId& commitHash, bool listIgnored) {
  return ObjectId(
      folly::fbstring(fmt::format("{}:{}", commitHash.value(), listIgnored)));
}

} // namespace facebook::eden
