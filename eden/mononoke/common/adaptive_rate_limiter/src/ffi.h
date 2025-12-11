/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/mononoke/common/adaptive_rate_limiter/cpp/AdaptiveRateLimiterConfig.h"
#include "eden/mononoke/common/adaptive_rate_limiter/cpp/AdaptiveRateLimiterWrapper.h"

namespace facebook::mononoke::ffi {

// Type aliases for FFI
using CppAdaptiveRateLimiterWrapper =
    facebook::mononoke::AdaptiveRateLimiterWrapper;
using CppAdaptiveRateLimiterConfig =
    facebook::mononoke::AdaptiveRateLimiterConfig;
using CppResourceMonitoringMode = facebook::mononoke::ResourceMonitoringMode;
using CppOperationMode = facebook::mononoke::OperationMode;

// Factory function to create rate limiter
std::unique_ptr<CppAdaptiveRateLimiterWrapper> new_adaptive_rate_limiter(
    const CppAdaptiveRateLimiterConfig& config);

// Helper function to create configuration from Rust
std::unique_ptr<CppAdaptiveRateLimiterConfig> make_config(
    uint32_t operation_mode,
    uint32_t monitoring_mode,
    double cpu_soft,
    double cpu_hard,
    double mem_soft,
    double mem_hard,
    uint64_t update_period_ms);

// Wrapper functions to match Rust naming convention
bool should_shed(const CppAdaptiveRateLimiterWrapper& limiter);
void update_config(
    CppAdaptiveRateLimiterWrapper& limiter,
    const CppAdaptiveRateLimiterConfig& config);

} // namespace facebook::mononoke::ffi
