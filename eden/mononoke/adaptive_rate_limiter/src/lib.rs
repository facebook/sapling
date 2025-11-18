/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adaptive Rate Limiter for Rust services
//!
//! This library provides load shedding capabilities based on CPU and memory utilization,
//! wrapping Meta's C++ Adaptive Rate Limiter with stats counter integration.

pub mod ffi;

use stats::prelude::*;

define_stats! {
    prefix = "mononoke.arl";
    total_requests: dynamic_timeseries("{}.total_requests", (service: String); Rate, Sum),
    accepted_requests: dynamic_timeseries("{}.accepted_requests", (service: String); Rate, Sum),
    rejected_requests: dynamic_timeseries("{}.rejected_requests", (service: String); Rate, Sum),
}

/// Resource monitoring mode for the rate limiter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceMonitoringMode {
    /// No resource monitoring - rate limiter disabled
    None,
    /// Monitor only cgroup (container) resources
    CgroupOnly,
    /// Monitor only host-level resources
    HostOnly,
    /// Monitor both cgroup and host resources
    Both,
}

impl From<ResourceMonitoringMode> for u32 {
    fn from(mode: ResourceMonitoringMode) -> Self {
        match mode {
            ResourceMonitoringMode::None => 0,
            ResourceMonitoringMode::CgroupOnly => 1,
            ResourceMonitoringMode::HostOnly => 2,
            ResourceMonitoringMode::Both => 3,
        }
    }
}

/// Operation mode for adaptive rate limiting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationMode {
    /// Rate limiter is disabled - no monitoring or shedding
    Disabled,
    /// Rate limiter is enabled - actively shed requests when limits are exceeded
    Enabled,
    /// Dry run mode - monitor and log what would be shed, but don't actually shed
    DryRun,
}

impl From<OperationMode> for u32 {
    fn from(mode: OperationMode) -> Self {
        match mode {
            OperationMode::Disabled => 0,
            OperationMode::Enabled => 1,
            OperationMode::DryRun => 2,
        }
    }
}

/// Configuration for the Adaptive Rate Limiter
#[derive(Debug, Clone)]
pub struct AdaptiveRateLimiterConfig {
    /// Operation mode - controls whether rate limiter is enabled/disabled/dry-run
    pub operation_mode: OperationMode,
    /// Resource monitoring mode
    pub monitoring_mode: ResourceMonitoringMode,
    /// CPU soft limit ratio (0.0 to 1.0)
    pub cpu_soft_limit_ratio: f64,
    /// CPU hard limit ratio (0.0 to 1.0)
    pub cpu_hard_limit_ratio: f64,
    /// Memory soft limit ratio (0.0 to 1.0)
    pub mem_soft_limit_ratio: f64,
    /// Memory hard limit ratio (0.0 to 1.0)
    pub mem_hard_limit_ratio: f64,
    /// Load update period in milliseconds
    pub load_update_period_ms: u64,
}

impl AdaptiveRateLimiterConfig {
    fn to_cpp_config(&self) -> cxx::UniquePtr<ffi::ffi::CppAdaptiveRateLimiterConfig> {
        ffi::ffi::make_config(
            self.operation_mode.into(),
            self.monitoring_mode.into(),
            self.cpu_soft_limit_ratio,
            self.cpu_hard_limit_ratio,
            self.mem_soft_limit_ratio,
            self.mem_hard_limit_ratio,
            self.load_update_period_ms,
        )
    }
}

/// Adaptive Rate Limiter with stats counter integration
pub struct AdaptiveRateLimiter {
    inner: cxx::UniquePtr<ffi::ffi::CppAdaptiveRateLimiterWrapper>,
    service_name: String,
}

impl AdaptiveRateLimiter {
    /// Create a new adaptive rate limiter with the given configuration and service name
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for CPU/memory thresholds and monitoring mode
    /// * `service_name` - Service name for stats counter prefix (e.g., "location_service")
    ///
    /// # Counter Names
    ///
    /// The following stats counters will be created:
    /// - `arl.{service_name}.total_requests`
    /// - `arl.{service_name}.accepted_requests`
    /// - `arl.{service_name}.rejected_requests`
    pub fn new(config: AdaptiveRateLimiterConfig, service_name: impl Into<String>) -> Self {
        let cpp_config = config.to_cpp_config();
        let inner = ffi::ffi::new_adaptive_rate_limiter(&cpp_config);
        Self {
            inner,
            service_name: service_name.into(),
        }
    }

    /// Check if the current request should be shed (rejected)
    ///
    /// This method increments stats counters and calls the underlying C++ rate limiter.
    pub fn should_shed(&self) -> bool {
        STATS::total_requests.add_value(1, (self.service_name.clone(),));
        let should_shed = ffi::ffi::should_shed(&self.inner);
        if should_shed {
            STATS::rejected_requests.add_value(1, (self.service_name.clone(),));
        } else {
            STATS::accepted_requests.add_value(1, (self.service_name.clone(),));
        }
        should_shed
    }

    /// Update the configuration at runtime
    pub fn update_config(&mut self, config: AdaptiveRateLimiterConfig) {
        let cpp_config = config.to_cpp_config();
        ffi::ffi::update_config(self.inner.pin_mut(), &cpp_config);
    }
}

// Thread safety: The underlying C++ rate limiter is thread-safe
unsafe impl Send for AdaptiveRateLimiter {}
unsafe impl Sync for AdaptiveRateLimiter {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_create_rate_limiter() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let _limiter = AdaptiveRateLimiter::new(config, "test_service");
    }

    #[mononoke::test]
    fn test_should_shed_increments_counters() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_counters");

        // Call should_shed a few times - counters will be exported to ODS
        for _ in 0..5 {
            let _ = limiter.should_shed();
        }
    }

    #[mononoke::test]
    fn test_monitoring_mode_none_never_sheds() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::None,
            cpu_soft_limit_ratio: 0.0,
            cpu_hard_limit_ratio: 0.0,
            mem_soft_limit_ratio: 0.0,
            mem_hard_limit_ratio: 0.0,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_none_mode");

        // When monitoring is disabled, should never shed
        for _ in 0..10 {
            assert!(!limiter.should_shed());
        }
    }

    #[mononoke::test]
    fn test_update_config() {
        let initial_config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let mut limiter = AdaptiveRateLimiter::new(initial_config, "test_update");

        let strict_config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::Both,
            cpu_soft_limit_ratio: 0.5,
            cpu_hard_limit_ratio: 0.7,
            mem_soft_limit_ratio: 0.6,
            mem_hard_limit_ratio: 0.8,
            load_update_period_ms: 50,
        };
        limiter.update_config(strict_config);
        let _ = limiter.should_shed();
    }

    #[mononoke::test]
    fn test_all_monitoring_modes() {
        let modes = vec![
            ResourceMonitoringMode::None,
            ResourceMonitoringMode::CgroupOnly,
            ResourceMonitoringMode::HostOnly,
            ResourceMonitoringMode::Both,
        ];
        for (i, mode) in modes.into_iter().enumerate() {
            let config = AdaptiveRateLimiterConfig {
                operation_mode: OperationMode::Enabled,
                monitoring_mode: mode,
                cpu_soft_limit_ratio: 0.7,
                cpu_hard_limit_ratio: 0.85,
                mem_soft_limit_ratio: 0.8,
                mem_hard_limit_ratio: 0.95,
                load_update_period_ms: 100,
            };
            let limiter = AdaptiveRateLimiter::new(config, format!("test_mode_{}", i));
            let _ = limiter.should_shed();
        }
    }

    #[mononoke::test]
    fn test_multiple_invocations() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_multiple");

        // Call many times - counters will be exported to ODS
        for _ in 0..100 {
            let _ = limiter.should_shed();
        }
    }

    #[mononoke::test]
    fn test_thread_safety() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let limiter = Arc::new(AdaptiveRateLimiter::new(config, "test_threads"));

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let limiter_clone = Arc::clone(&limiter);
                mononoke::spawn_thread(move || {
                    for _ in 0..25 {
                        let _ = limiter_clone.should_shed();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[mononoke::test]
    fn test_counter_prefix_isolation() {
        // Create two limiters with different service names
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };

        let limiter1 = AdaptiveRateLimiter::new(config.clone(), "service_a");
        let limiter2 = AdaptiveRateLimiter::new(config, "service_b");

        // Call should_shed different amounts on each - counters exported independently
        for _ in 0..3 {
            let _ = limiter1.should_shed();
        }
        for _ in 0..7 {
            let _ = limiter2.should_shed();
        }
    }

    /// CPU stress test: Generate load for 5 seconds with very low limits
    #[mononoke::test]
    fn test_cpu_stress_with_low_limits() {
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.1,
            cpu_hard_limit_ratio: 0.2,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let limiter = Arc::new(AdaptiveRateLimiter::new(config, "test_stress"));

        let stress_handles: Vec<_> = (0..4)
            .map(|_| {
                mononoke::spawn_thread(move || {
                    let start = std::time::Instant::now();
                    while start.elapsed().as_secs() < 5 {
                        let _result = (0..1000).fold(0u64, |acc, x| acc.wrapping_add(x * x));
                    }
                })
            })
            .collect();

        std::thread::sleep(std::time::Duration::from_millis(500));

        let mut accepted = 0;
        let mut rejected = 0;
        let test_start = std::time::Instant::now();

        while test_start.elapsed().as_secs() < 4 {
            if limiter.should_shed() {
                rejected += 1;
            } else {
                accepted += 1;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        for handle in stress_handles {
            handle.join().unwrap();
        }

        println!(
            "Stress test: {} accepted, {} rejected, {:.1}% rejection rate",
            accepted,
            rejected,
            (rejected as f64 / (accepted + rejected) as f64) * 100.0
        );
    }

    #[mononoke::test]
    fn test_operation_mode_disabled() {
        // Test that DISABLED mode never sheds
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Disabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.0, // Very strict thresholds
            cpu_hard_limit_ratio: 0.0,
            mem_soft_limit_ratio: 0.0,
            mem_hard_limit_ratio: 0.0,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_disabled");

        // Should never shed when operation mode is DISABLED
        for _ in 0..10 {
            assert!(!limiter.should_shed());
        }
    }

    #[mononoke::test]
    fn test_operation_mode_dry_run() {
        // Test that DRY_RUN mode never actually sheds
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::DryRun,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.0, // Very strict thresholds - would shed if enabled
            cpu_hard_limit_ratio: 0.0,
            mem_soft_limit_ratio: 0.0,
            mem_hard_limit_ratio: 0.0,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_dry_run");

        // Should never shed in DRY_RUN mode (logs but doesn't shed)
        for _ in 0..10 {
            assert!(!limiter.should_shed());
        }
    }

    #[mononoke::test]
    fn test_operation_mode_enabled() {
        // Test that ENABLED mode can shed
        let config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };
        let limiter = AdaptiveRateLimiter::new(config, "test_enabled");

        // Should be able to call should_shed
        // Actual shedding depends on system load
        let _ = limiter.should_shed();
    }

    #[mononoke::test]
    fn test_update_operation_mode() {
        // Test changing operation mode at runtime
        let initial_config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Disabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.0,
            cpu_hard_limit_ratio: 0.0,
            mem_soft_limit_ratio: 0.0,
            mem_hard_limit_ratio: 0.0,
            load_update_period_ms: 100,
        };

        let mut limiter = AdaptiveRateLimiter::new(initial_config, "test_update_op_mode");

        // Initially DISABLED - should never shed
        assert!(!limiter.should_shed());

        // Change to DRY_RUN mode
        let dry_run_config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::DryRun,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.0,
            cpu_hard_limit_ratio: 0.0,
            mem_soft_limit_ratio: 0.0,
            mem_hard_limit_ratio: 0.0,
            load_update_period_ms: 100,
        };

        limiter.update_config(dry_run_config);

        // DRY_RUN should also not shed
        assert!(!limiter.should_shed());

        // Change to ENABLED mode
        let enabled_config = AdaptiveRateLimiterConfig {
            operation_mode: OperationMode::Enabled,
            monitoring_mode: ResourceMonitoringMode::CgroupOnly,
            cpu_soft_limit_ratio: 0.7,
            cpu_hard_limit_ratio: 0.85,
            mem_soft_limit_ratio: 0.8,
            mem_hard_limit_ratio: 0.95,
            load_update_period_ms: 100,
        };

        limiter.update_config(enabled_config);

        // ENABLED mode - can shed based on system load
        let _ = limiter.should_shed();
    }
}
