/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenStats.h"

#include <chrono>
#include <memory>

namespace facebook::eden {

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

ThriftThreadStats& EdenStats::getThriftStatsForCurrentThread() {
  return *threadLocalThriftStats_.get();
}

void EdenStats::flush() {
  // This method is only really useful while testing to ensure that the service
  // data singleton instance has the latest stats. Since all our stats are now
  // quantile stat based, flushing the quantile stat map is sufficient for that
  // use case.
  fb303::ServiceData::get()->getQuantileStatMap()->flushAll();
}

std::shared_ptr<HgImporterThreadStats> getSharedHgImporterStatsForCurrentThread(
    std::shared_ptr<EdenStats> stats) {
  return std::shared_ptr<HgImporterThreadStats>(
      stats, &stats->getHgImporterStatsForCurrentThread());
}

EdenThreadStatsBase::DurationStat::DurationStat(std::string_view name)
    : Stat{
          name,
          fb303::ExportTypeConsts::kSumCountAvgRate,
          fb303::QuantileConsts::kP1_P10_P50_P90_P99,
          fb303::SlidingWindowPeriodConsts::kOneMinTenMinHour} {
  // This should be a compile-time check but I don't know how to spell that in a
  // convenient way. :)
  assert(name.size() > 3 && "duration name too short");
  assert(
      (std::string_view{name.data() + name.size() - 3, 3}) == "_us" &&
      "duration stats must end in _us");
}

void EdenThreadStatsBase::DurationStat::addDuration(
    std::chrono::microseconds elapsed) {
  addValue(elapsed.count());
}

EdenThreadStatsBase::Stat EdenThreadStatsBase::createStat(
    std::string_view name) {
  return Stat{
      name,
      fb303::ExportTypeConsts::kSumCountAvgRate,
      fb303::QuantileConsts::kP1_P10_P50_P90_P99,
      fb303::SlidingWindowPeriodConsts::kOneMinTenMinHour,
  };
}

} // namespace facebook::eden
