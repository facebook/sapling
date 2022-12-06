/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <optional>
#include <string>
#include <unordered_map>

#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

struct FetchStatistics {
  struct Access {
    /**
     * Total number of object accesses, including cache hits.
     */
    uint64_t accessCount = 0;

    /**
     * Number of object fetches from the backing store.
     */
    uint64_t fetchCount = 0;

    /**
     * In range [0, 100]. unsigned char is big enough, but prints as a
     * character.
     */
    unsigned short cacheHitRate = 0;
  };

  Access tree;
  Access blob;
  Access metadata;
};

class StatsFetchContext : public ObjectFetchContext {
 public:
  StatsFetchContext() = default;
  StatsFetchContext(
      std::optional<pid_t> pid,
      Cause cause,
      std::string_view causeDetail,
      const std::unordered_map<std::string, std::string>* requestInfo);
  StatsFetchContext(const StatsFetchContext& other);

  // TODO: When ObjectFetchContext is passed by refcounted pointer, make this
  // non-moveable.
  StatsFetchContext(StatsFetchContext&& other) noexcept;
  StatsFetchContext& operator=(StatsFetchContext&&) noexcept;

  void didFetch(ObjectType type, const ObjectId& id, Origin origin) override;

  std::optional<pid_t> getClientPid() const override;

  Cause getCause() const override;

  std::optional<std::string_view> getCauseDetail() const override;

  uint64_t countFetchesOfType(ObjectType type) const;
  uint64_t countFetchesOfTypeAndOrigin(ObjectType type, Origin origin) const;

  FetchStatistics computeStatistics() const;

  /**
   * Sums the counts from another fetch context into this one.
   */
  void merge(const StatsFetchContext& other);

  const std::unordered_map<std::string, std::string>* getRequestInfo()
      const override {
    return &requestInfo_;
  }

 private:
  std::atomic<uint64_t> counts_[ObjectFetchContext::kObjectTypeEnumMax]
                               [ObjectFetchContext::kOriginEnumMax] = {};
  std::optional<pid_t> clientPid_ = std::nullopt;
  Cause cause_ = Cause::Unknown;
  std::string_view causeDetail_;
  std::unordered_map<std::string, std::string> requestInfo_;
};

using StatsFetchContextPtr = RefPtr<StatsFetchContext>;

} // namespace facebook::eden
