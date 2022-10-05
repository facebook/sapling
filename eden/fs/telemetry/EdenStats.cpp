/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenStats.h"

#include <folly/logging/xlog.h>
#include <chrono>
#include <memory>

namespace facebook::eden {

void EdenStats::flush() {
  // This method is only really useful while testing to ensure that the service
  // data singleton instance has the latest stats. Since all our stats are now
  // quantile stat based, flushing the quantile stat map is sufficient for that
  // use case.
  fb303::ServiceData::get()->getQuantileStatMap()->flushAll();
}

StatsGroupBase::Counter::Counter(std::string_view name)
    : Stat{
          name,
          fb303::ExportTypeConsts::kSumCountAvgRate,
          // Don't record quantiles for counters. Usually "1" is the only value
          // added. Usually we care about counts and rates.
          {},
          fb303::SlidingWindowPeriodConsts::kOneMinTenMinHour,
      } {
  // TODO: enforce the name matches the StatsGroup prefix.
}

StatsGroupBase::Duration::Duration(std::string_view name)
    : Stat{
          name,
          fb303::ExportTypeConsts::kSumCountAvgRate,
          fb303::QuantileConsts::kP1_P10_P50_P90_P99,
          fb303::SlidingWindowPeriodConsts::kOneMinTenMinHour} {
  // This should be a compile-time check but I don't know how to spell that in a
  // convenient way. :) Asserting at startup in debug mode should be sufficient.
  XCHECK_GT(name.size(), size_t{3}) << "duration name too short";
  XCHECK_EQ("_us", std::string_view(name.data() + name.size() - 3, 3))
      << "duration stats must end in _us";
  // TODO: enforce the name matches the StatsGroup prefix.
}

void StatsGroupBase::Duration::addDuration(std::chrono::microseconds elapsed) {
  addValue(elapsed.count());
}

DurationScope::~DurationScope() noexcept {
  if (edenStats_ && updateScope_) {
    try {
      updateScope_(*edenStats_, stopWatch_.elapsed());
    } catch (const std::exception& e) {
      XLOG(ERR) << "error recording duration: " << e.what();
    }
  }
}

} // namespace facebook::eden
