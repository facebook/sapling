/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/ObjectCache.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

class ReloadableConfig;

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
    for (const auto& entry : status.entries_ref().value()) {
      statusSize += entry.first.size() * sizeof(char) + sizeof(entry.second);
    }
    return internalSize + statusSize;
  }
};

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
 public:
  static std::shared_ptr<ScmStatusCache> create(
      const EdenConfig* config,
      EdenStatsPtr stats) {
    return std::make_shared<ScmStatusCache>(config, std::move(stats));
  }
  ScmStatusCache(const EdenConfig* configPtr, EdenStatsPtr stats);

  static ObjectId makeKey(const RootId& commitHash, bool listIgnored);

  std::shared_ptr<const SeqStatusPair> get(const ObjectId& hash);
  void insert(ObjectId id, std::shared_ptr<const SeqStatusPair> status);
};

} // namespace facebook::eden
