/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ScmStatusCache.h"
#include <folly/logging/xlog.h>
#include "eden/fs/journal/Journal.h"

namespace facebook::eden {

std::shared_ptr<ScmStatusCache> ScmStatusCache::create(
    const EdenConfig* config,
    EdenStatsPtr stats,
    std::shared_ptr<Journal> journal) {
  return std::make_shared<ScmStatusCache>(
      config, std::move(stats), std::move(journal));
}

ScmStatusCache::ScmStatusCache(
    const EdenConfig* configPtr,
    EdenStatsPtr stats,
    std::shared_ptr<Journal> journal)
    : ObjectCache<
          SeqStatusPair,
          ObjectCacheFlavor::Simple,
          ScmStatusCacheStats>{
          configPtr->scmStatusCacheMaxSize.getValue(),
          configPtr->scmStatusCacheMinimumItems.getValue(),
          std::move(stats)}, journal_(std::move(journal)) {}

std::variant<StatusResultFuture, StatusResultPromise> ScmStatusCache::get(
    const ObjectId& key,
    JournalDelta::SequenceNumber curSeq) {
  auto internalCachedItem = getSimple(key);
  if (internalCachedItem && isSequenceValid(curSeq, internalCachedItem->seq)) {
    XLOGF(
        DBG7,
        "hit internal cache: key={}, curSeq={}, cachedSeq={}",
        key,
        curSeq,
        internalCachedItem->seq);
    internalCachedItem->seq =
        curSeq; // update seq so we can avoid calculating the same range again
    return ImmediateFuture<ScmStatus>{internalCachedItem->status};
  }

  auto it = promiseMap_.find(key);
  if (it != promiseMap_.end() && isSequenceValid(curSeq, it->second.first)) {
    XLOGF(
        DBG7,
        "hit promise map: key={}, curSeq={}, cachedSeq={}",
        key,
        curSeq,
        it->second.first);
    it->second.first =
        curSeq; // update seq so we can avoid calculating the same range again
    return it->second.second->getFuture();
  }

  auto promise = std::make_shared<folly::SharedPromise<ScmStatus>>();
  promiseMap_.insert_or_assign(key, std::make_pair(curSeq, promise));

  XLOGF(DBG7, "cache miss: key={}, curSeq={}", key, curSeq);
  return promise;
}

void ScmStatusCache::insert(
    ObjectId key,
    JournalDelta::SequenceNumber curSeq,
    ScmStatus status) {
  auto internalCachedItem = getSimple(key);

  if (!internalCachedItem) {
    insertSimple(
        std::move(key),
        std::make_shared<SeqStatusPair>(curSeq, std::move(status)));
    return;
  }

  // It's only necessary to update the cache if the diff is computed
  // for a larger sequenceID than the existing one.
  if (curSeq > internalCachedItem->seq) {
    invalidate(key);
    insertSimple(
        std::move(key),
        std::make_shared<SeqStatusPair>(curSeq, std::move(status)));
  }
}

void ScmStatusCache::dropPromise(
    const ObjectId& key,
    JournalDelta::SequenceNumber curSeq) {
  auto it = promiseMap_.find(key);
  // we don't want to accidentally drop promises owned by other requests
  // which query with a larger sequence number
  if (it != promiseMap_.end() && it->second.first == curSeq) {
    promiseMap_.erase(key);
  }
}

ObjectId ScmStatusCache::makeKey(const RootId& commitId, bool listIgnored) {
  return ObjectId(
      folly::fbstring(fmt::format("{}:{}", commitId.value(), listIgnored)));
}

bool ScmStatusCache::isSequenceValid(
    JournalDelta::SequenceNumber curSeq,
    JournalDelta::SequenceNumber cachedSeq) const {
  if (cachedSeq >= curSeq) {
    return true;
  }

  // There is a chance that the latest sequence of the journal is larger than
  // the current sequence.
  // This is OK because when calculating the range, the final range will
  // include our desired range. So if the final range does not contain non-.hg
  // changes, we are sure that the current sequence is valid.
  auto range = journal_->accumulateRange(
      cachedSeq + 1); // plus one because the range for calculation is inclusive
  bool valid = !range->isTruncated && range->containsHgOnlyChanges &&
      !range->containsRootUpdate;

  XLOGF(
      DBG7,
      "range: from={}, truncated={}, hgOnly={}, rootUpdate={}",
      cachedSeq,
      range->isTruncated,
      range->containsHgOnlyChanges,
      range->containsRootUpdate);
  return valid;
}

void ScmStatusCache::clear() {
  XLOGF(
      DBG7,
      "clearing cache: cachedRoot={}, cacheSize={}",
      cachedWorkingCopyParentRootId_.value(),
      getObjectCount());
  ObjectCache::clear();
  promiseMap_.clear(); // safe to clear because we know the promise is
                       // referenced by at least one pending request
  resetCachedWorkingDir();
}

bool ScmStatusCache::isCachedWorkingDirValid(RootId& curWorkingDir) const {
  XLOGF(
      DBG7,
      "cachedRoot={}, currentRoot={}",
      cachedWorkingCopyParentRootId_.value(),
      curWorkingDir.value());
  return cachedWorkingCopyParentRootId_ == curWorkingDir;
}

void ScmStatusCache::resetCachedWorkingDir(RootId curWorkingDir) {
  cachedWorkingCopyParentRootId_ = std::move(curWorkingDir);
}

} // namespace facebook::eden
