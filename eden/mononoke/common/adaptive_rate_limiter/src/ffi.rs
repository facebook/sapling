/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cxx::bridge(namespace = "facebook::mononoke::ffi")]
pub mod ffi {
    unsafe extern "C++" {
        include!("eden/mononoke/common/adaptive_rate_limiter/src/ffi.h");

        // Opaque C++ types
        type CppAdaptiveRateLimiterWrapper;
        type CppAdaptiveRateLimiterConfig;
        type CppResourceMonitoringMode;

        // Factory function
        fn new_adaptive_rate_limiter(
            config: &CppAdaptiveRateLimiterConfig,
        ) -> UniquePtr<CppAdaptiveRateLimiterWrapper>;

        // Config builder function
        fn make_config(
            operation_mode: u32,
            monitoring_mode: u32,
            cpu_soft: f64,
            cpu_hard: f64,
            mem_soft: f64,
            mem_hard: f64,
            update_period_ms: u64,
        ) -> UniquePtr<CppAdaptiveRateLimiterConfig>;

        // Rate limiter functions (standalone, not methods)
        fn should_shed(limiter: &CppAdaptiveRateLimiterWrapper) -> bool;

        fn update_config(
            limiter: Pin<&mut CppAdaptiveRateLimiterWrapper>,
            config: &CppAdaptiveRateLimiterConfig,
        );
    }
}
