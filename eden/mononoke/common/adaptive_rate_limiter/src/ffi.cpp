/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/mononoke/common/adaptive_rate_limiter/src/ffi.h"

namespace facebook::mononoke::ffi {

std::unique_ptr<CppAdaptiveRateLimiterWrapper> new_adaptive_rate_limiter(
    const CppAdaptiveRateLimiterConfig& config) {
  return std::make_unique<CppAdaptiveRateLimiterWrapper>(config);
}

std::unique_ptr<CppAdaptiveRateLimiterConfig> make_config(
    uint32_t operation_mode,
    uint32_t monitoring_mode,
    double cpu_soft,
    double cpu_hard,
    double mem_soft,
    double mem_hard,
    uint64_t update_period_ms) {
  // Convert uint32_t to enum types
  auto op_mode = static_cast<CppOperationMode>(operation_mode);
  auto mon_mode = static_cast<CppResourceMonitoringMode>(monitoring_mode);

  return std::make_unique<CppAdaptiveRateLimiterConfig>(
      op_mode,
      mon_mode,
      cpu_soft,
      cpu_hard,
      mem_soft,
      mem_hard,
      update_period_ms);
}

bool should_shed(const CppAdaptiveRateLimiterWrapper& limiter) {
  // Cast away constness because the underlying shouldShed() method is not const
  // This is safe because shouldShed() is thread-safe and doesn't modify logical
  // state
  return const_cast<CppAdaptiveRateLimiterWrapper&>(limiter).shouldShed();
}

void update_config(
    CppAdaptiveRateLimiterWrapper& limiter,
    const CppAdaptiveRateLimiterConfig& config) {
  limiter.updateConfig(config);
}

} // namespace facebook::mononoke::ffi
