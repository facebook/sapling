/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenStats.h"

#include <chrono>
#include <memory>

namespace facebook {
namespace eden {

ChannelThreadStats& EdenStats::getChannelStatsForCurrentThread() {
  return *threadLocalChannelStats_.get();
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
  // Flush the quantile stats since some of our stats are based on that
  // mechanism. Eventually, every stat will be a quantile stat and we can
  // remove the rest of the logic from this method.
  fb303::ServiceData::get()->getQuantileStatMap()->flushAll();

  for (auto& stats : threadLocalChannelStats_.accessAllThreads()) {
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

EdenThreadStatsBase::Stat EdenThreadStatsBase::createStat(
    const std::string& name) {
  return Stat{
      name,
      fb303::ExportTypeConsts::kSumCountAvgRate,
      fb303::QuantileConsts::kP1_P10_P50_P90_P99,
      fb303::SlidingWindowPeriodConsts::kOneMinTenMinHour,
  };
}

EdenThreadStatsBase::Timeseries EdenThreadStatsBase::createTimeseries(
    const std::string& name) {
  auto timeseries = Timeseries{this, name};
  timeseries.exportStat(fb303::COUNT);
  timeseries.exportStat(fb303::PERCENT);
  return timeseries;
}

void ChannelThreadStats::recordLatency(
    StatPtr item,
    std::chrono::microseconds elapsed) {
  (this->*item).addValue(elapsed.count());
}

} // namespace eden
} // namespace facebook
