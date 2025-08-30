/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/container/EvictingCacheMap.h>
#include <folly/futures/Future.h>
#include <folly/futures/SharedPromise.h>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectCache.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

class ReloadableConfig;
class Journal;

/**
 * We only store one journal position per status parameters, because journal
 * positions only move forward, clients in future calls should happen with
 * equal or greater journal positions. There is no point storing an older
 * journal position if we have the result for a newer one because clients will
 * never want results from the older journal position.
 */
struct SeqStatusPair {
  mutable JournalDelta::SequenceNumber seq;
  mutable ScmStatus status;

  SeqStatusPair(JournalDelta::SequenceNumber seq, ScmStatus status)
      : seq(seq), status(std::move(status)) {}

  size_t getSizeBytes() const {
    size_t internalSize = sizeof(*this);
    size_t statusSize = 0;
    for (const auto& entry : status.entries().value()) {
      statusSize += entry.first.size() * sizeof(char) + sizeof(entry.second);
    }
    return internalSize + statusSize;
  }
};

using StatusResultFuture = ImmediateFuture<ScmStatus>;
using StatusResultPromise = std::shared_ptr<folly::SharedPromise<ScmStatus>>;

/**
 * Cache for ScmStatus results. Used by EdenMount.
 *
 * Note: This cache implementation is not thread safe.
 * It can only be interacted with one thread at a time.
 */
class ScmStatusCache : public ObjectCache<
                           SeqStatusPair,
                           ObjectCacheFlavor::Simple,
                           ScmStatusCacheStats> {
  using StatusPromise = std::shared_ptr<folly::SharedPromise<ScmStatus>>;
  using PromiseMapValue =
      std::pair<JournalDelta::SequenceNumber, StatusPromise>;

 public:
  static std::shared_ptr<ScmStatusCache> create(
      const EdenConfig* config,
      EdenStatsPtr stats,
      std::shared_ptr<Journal> journal);

  ScmStatusCache(
      const EdenConfig* configPtr,
      EdenStatsPtr stats,
      std::shared_ptr<Journal> journal);
  static ObjectId makeKey(const RootId& commitId, bool listIgnored);

  /**
   * Query the cache and see if we can reuse an existing result.
   *
   * Returns a future or a promise.
   *
   * Future: If there is a pending request(other than the caller) has
   * or is computing the same status result.
   *
   * Promise: If the caller itself should compute the status result.
   * The caller should fufil the promise when done as well as call dropPromise
   * to cleanup the promise itself.
   *
   * Any returned future will be a valid future.
   * Any returned promise will be valid (no nullptr)
   *
   * First we check the internal cache. If the key exists and the cached
   * sequence number is larger than the current sequence number, we reuse
   * the result - returning a ready future.
   * Otherwise, check the promise map. If the key exists and the
   * sequence number is larger than the current sequence number, we can
   * return the stored future. If no luck, overwrite the promise map with a new
   * promise and indicate the caller by returning a result as the new promise.
   *
   * Note: The reason why it's OK to reuse the cached result when the cached
   * sequence number is larger than the current sequence number is because a
   * larger sequence number indicates a later point in time, thus a newer result
   * already cached.
   *
   * Note: It's always safe to overwrite the promise map entry because a
   * reference to the promise should always be held by a caller.
   */
  std::variant<StatusResultFuture, StatusResultPromise> get(
      const ObjectId& key,
      JournalDelta::SequenceNumber curSeq);

  /**
   * Insert a new result into the internal cache.
   *
   * Note: The Caller should not worry about the logic of when to insert. The
   * cache implementation should check if an insert is actually needed.
   *
   * There are two cases when we should perform the insert operation:
   * 1. If the key does not exist - Obviously
   * 2. If the key exists but the cached sequence number is smaller than the
   *    current sequence number. This is because a larger sequence number
   *    indicates a later point in time and we want to keep our cache up to
   *    date.
   */
  void
  insert(ObjectId key, JournalDelta::SequenceNumber curSeq, ScmStatus status);

  /**
   * Drop the promise for a given key and sequence number from the promise map.
   *
   * Note: we use a dedicated method for this instead of dropping inside insert
   * because we want to ensure the promise is dropped even in the error cases to
   * avoid increasing the promise map to an unbounded size.
   */
  void dropPromise(const ObjectId& key, JournalDelta::SequenceNumber seq);

  /**
   * Clear this cache so both promiseMap and the internal ObjectCache are empty.
   */
  void clear();

  /**
   * Check if the cached entry is with a sequence number that is valid
   * to reuse given the current sequence number.
   */
  bool isSequenceValid(
      JournalDelta::SequenceNumber curSeq,
      JournalDelta::SequenceNumber cachedSeq) const;

  /**
   * Check if the cached working copy parent root id is valid to reuse given
   * the current working copy parent root id.
   */
  bool isCachedWorkingDirValid(RootId& curWorkingDir) const;

  /**
   * Reset the cached working copy parent root id.
   * Clear it if no RootId provided.
   */
  void resetCachedWorkingDir(RootId curWorkingDir = RootId());

 private:
  /**
   * A map of promises that are waiting for a result when given a key.
   * Only the thread which does the actual computation of the diff should be
   * setting the value of a promise.
   * And the entry should be removed after the promise is fulfilled and the
   * result is inserted into the internal cache.
   */
  std::unordered_map<ObjectId, PromiseMapValue> promiseMap_;

  /**
   * The cached working copy parent root id. This is used to determine if this
   * cache is valid to use to fetch a cached diff result for current working
   * copy.
   */
  RootId cachedWorkingCopyParentRootId_;

  /**
   * Use journal to determine if the sequence range contains changes outside
   * the ".hg" folder. If so, it means the cache is not safe to reuse.
   */
  std::shared_ptr<Journal> journal_;
};

} // namespace facebook::eden
