/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/tracing/EdenStats.h"

#include <folly/container/Array.h>
#include <chrono>
#include <memory>

#include "eden/fs/eden-config.h"

using namespace folly;
using namespace std::chrono;

namespace {
constexpr std::chrono::microseconds kMinValue{0};
constexpr std::chrono::microseconds kMaxValue{10000};
constexpr std::chrono::microseconds kBucketSize{1000};
} // namespace

namespace facebook {
namespace eden {

FuseThreadStats& EdenStats::getFuseStatsForCurrentThread() {
  return *threadLocalFuseStats_.get();
}

ObjectStoreThreadStats& EdenStats::getObjectStoreStatsForCurrentThread() {
  return *threadLocalObjectStoreStats_.get();
}

HgBackingStoreThreadStats& EdenStats::getHgBackingStoreStatsForCurrentThread() {
  return *threadLocalHgBackingStoreStats_.get();
}

HgImporterThreadStats& EdenStats::getHgImporterStatsForCurrentThread() {
  return *threadLocalHgImporterStats_.get();
}

JournalThreadStats& EdenStats::getJournalStatsForCurrentThread() {
  return *threadLocalJournalStats_.get();
}

void EdenStats::aggregate() {
  for (auto& stats : threadLocalFuseStats_.accessAllThreads()) {
    stats.aggregate();
  }
  for (auto& stats : threadLocalObjectStoreStats_.accessAllThreads()) {
    stats.aggregate();
  }
  for (auto& stats : threadLocalHgBackingStoreStats_.accessAllThreads()) {
    stats.aggregate();
  }
  for (auto& stats : threadLocalHgImporterStats_.accessAllThreads()) {
    stats.aggregate();
  }
  for (auto& stats : threadLocalJournalStats_.accessAllThreads()) {
    stats.aggregate();
  }
}

std::shared_ptr<HgImporterThreadStats> getSharedHgImporterStatsForCurrentThread(
    std::shared_ptr<EdenStats> stats) {
  return std::shared_ptr<HgImporterThreadStats>(
      stats, &stats->getHgImporterStatsForCurrentThread());
}

EdenThreadStatsBase::EdenThreadStatsBase() {}

EdenThreadStatsBase::Histogram EdenThreadStatsBase::createHistogram(
    const std::string& name) {
  return Histogram{this,
                   name,
                   static_cast<int64_t>(kBucketSize.count()),
                   kMinValue.count(),
                   kMaxValue.count(),
                   fb303::COUNT,
                   50,
                   90,
                   99};
}

EdenThreadStatsBase::Timeseries EdenThreadStatsBase::createTimeseries(
    const std::string& name) {
  auto timeseries = Timeseries{this, name};
  timeseries.exportStat(fb303::COUNT);
  timeseries.exportStat(fb303::PERCENT);
  return timeseries;
}

void FuseThreadStats::recordLatency(
    HistogramPtr item,
    std::chrono::microseconds elapsed,
    std::chrono::seconds now) {
  (void)now; // we don't use it in this code path
  (this->*item).addValue(elapsed.count());
}

} // namespace eden
} // namespace facebook
