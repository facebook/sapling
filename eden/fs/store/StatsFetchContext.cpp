/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/StatsFetchContext.h"
#include <folly/logging/xlog.h>

namespace facebook::eden {

StatsFetchContext::StatsFetchContext(
    std::optional<pid_t> pid,
    Cause cause,
    std::string_view causeDetail,
    const std::unordered_map<std::string, std::string>* requestInfo)
    : clientPid_{pid}, cause_{cause}, causeDetail_{std::move(causeDetail)} {
  if (requestInfo) {
    requestInfo_ = *requestInfo;
  }
}

StatsFetchContext::StatsFetchContext(const StatsFetchContext& other)
    : clientPid_{other.clientPid_},
      cause_{other.cause_},
      causeDetail_{other.causeDetail_},
      requestInfo_{other.requestInfo_} {
  for (size_t y = 0; y < ObjectFetchContext::kObjectTypeEnumMax; ++y) {
    for (size_t x = 0; x < ObjectFetchContext::kOriginEnumMax; ++x) {
      // This could almost certainly use a more relaxed memory ordering.
      counts_[y][x] = other.counts_[y][x].load();
    }
  }
}

StatsFetchContext::StatsFetchContext(StatsFetchContext&& other) noexcept
    : clientPid_{other.clientPid_},
      cause_{other.cause_},
      causeDetail_{other.causeDetail_},
      requestInfo_{std::move(other.requestInfo_)} {
  for (size_t y = 0; y < ObjectFetchContext::kObjectTypeEnumMax; ++y) {
    for (size_t x = 0; x < ObjectFetchContext::kOriginEnumMax; ++x) {
      // This could almost certainly use a more relaxed memory ordering.
      counts_[y][x] = other.counts_[y][x].load();
    }
  }
}

StatsFetchContext& StatsFetchContext::operator=(
    StatsFetchContext&& other) noexcept {
  clientPid_ = std::move(other.clientPid_);
  cause_ = std::move(other.cause_);
  causeDetail_ = std::move(other.causeDetail_);
  requestInfo_ = std::move(other.requestInfo_);
  for (size_t y = 0; y < ObjectFetchContext::kObjectTypeEnumMax; ++y) {
    for (size_t x = 0; x < ObjectFetchContext::kOriginEnumMax; ++x) {
      // This could almost certainly use a more relaxed memory ordering.
      counts_[y][x] = other.counts_[y][x].load();
    }
  }
  return *this;
}

void StatsFetchContext::didFetch(
    ObjectType type,
    const ObjectId&,
    Origin origin) {
  XCHECK(type < ObjectFetchContext::kObjectTypeEnumMax)
      << "type is out of range: " << type;
  XCHECK(origin < ObjectFetchContext::kOriginEnumMax)
      << "origin is out of range: " << type;
  counts_[type][origin].fetch_add(1, std::memory_order_acq_rel);
}

uint64_t StatsFetchContext::countFetchesOfType(ObjectType type) const {
  XCHECK(type < ObjectFetchContext::kObjectTypeEnumMax)
      << "type is out of range: " << type;
  uint64_t result = 0;
  for (unsigned origin = 0; origin < ObjectFetchContext::kOriginEnumMax;
       ++origin) {
    result += counts_[type][origin].load(std::memory_order_acquire);
  }
  return result;
}

void StatsFetchContext::merge(const StatsFetchContext& other) {
  for (unsigned type = 0; type < ObjectFetchContext::kObjectTypeEnumMax;
       ++type) {
    for (unsigned origin = 0; origin < ObjectFetchContext::kOriginEnumMax;
         ++origin) {
      counts_[type][origin] += other.counts_[type][origin];
    }
  }
}

uint64_t StatsFetchContext::countFetchesOfTypeAndOrigin(
    ObjectType type,
    Origin origin) const {
  XCHECK(type < ObjectFetchContext::kObjectTypeEnumMax)
      << "type is out of range: " << type;
  XCHECK(origin < ObjectFetchContext::kOriginEnumMax)
      << "origin is out of range: " << type;
  return counts_[type][origin].load(std::memory_order_acquire);
}

FetchStatistics StatsFetchContext::computeStatistics() const {
  auto computePercent = [&](uint64_t n, uint64_t d) -> unsigned short {
    XDCHECK_LE(n, d) << n << " > " << d;
    if (d == 0) {
      return 0;
    }
    return (1000 * n / d + 5) / 10;
  };

  auto computeAccessStats = [&](ObjectFetchContext::ObjectType type) {
    uint64_t fromMemory = counts_[type][ObjectFetchContext::FromMemoryCache];
    uint64_t fromDisk = counts_[type][ObjectFetchContext::FromDiskCache];
    uint64_t fromNetwork = counts_[type][ObjectFetchContext::FromNetworkFetch];
    uint64_t total = fromMemory + fromDisk + fromNetwork;
    return FetchStatistics::Access{
        total, fromNetwork, computePercent(fromMemory + fromDisk, total)};
  };

  auto result = FetchStatistics{};
  result.tree = computeAccessStats(ObjectFetchContext::Tree);
  result.blob = computeAccessStats(ObjectFetchContext::Blob);
  result.metadata = computeAccessStats(ObjectFetchContext::BlobMetadata);
  return result;
}

std::optional<pid_t> StatsFetchContext::getClientPid() const {
  return clientPid_;
}

ObjectFetchContext::Cause StatsFetchContext::getCause() const {
  return cause_;
}

std::optional<std::string_view> StatsFetchContext::getCauseDetail() const {
  return causeDetail_;
}

} // namespace facebook::eden
