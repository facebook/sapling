/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::mononoke {

/**
 * Resource monitoring mode for the rate limiter.
 * Determines what system resources are monitored for load shedding decisions.
 */
enum class ResourceMonitoringMode {
  // No resource monitoring - rate limiter is disabled
  NONE = 0,

  // Monitor only cgroup (container) resources
  // Uses per-cgroup CPU/memory limits
  // Best for containerized services (Tupperware)
  CGROUP_ONLY = 1,

  // Monitor only host-level resources
  // Uses entire host CPU/memory
  // Best for bare metal services
  HOST_ONLY = 2,

  // Monitor both cgroup and host resources
  // Sheds load if either cgroup OR host is under pressure
  // Most conservative - protects both container and host
  BOTH = 3,
};

/**
 * Configuration for the Adaptive Rate Limiter.
 * Defines CPU and memory thresholds for load shedding.
 */
struct AdaptiveRateLimiterConfig {
  // Resource monitoring mode
  ResourceMonitoringMode monitoringMode{ResourceMonitoringMode::CGROUP_ONLY};
  // CPU thresholds (0.0 to 1.0)
  // Soft limit: start shedding requests when CPU exceeds this
  // Hard limit: maximum shedding when CPU reaches this
  double cpuSoftLimitRatio{1.0};
  double cpuHardLimitRatio{1.0};

  // Memory thresholds (0.0 to 1.0)
  // Soft limit: start shedding requests when memory exceeds this
  // Hard limit: maximum shedding when memory reaches this
  double memSoftLimitRatio{1.0};
  double memHardLimitRatio{1.0};

  // Load update period in milliseconds
  // How frequently to refresh resource metrics
  uint64_t loadUpdatePeriodMs{100};

  AdaptiveRateLimiterConfig() = default;

  AdaptiveRateLimiterConfig(
      ResourceMonitoringMode mode,
      double cpuSoft,
      double cpuHard,
      double memSoft,
      double memHard,
      uint64_t updatePeriodMs = 100)
      : monitoringMode(mode),
        cpuSoftLimitRatio(cpuSoft),
        cpuHardLimitRatio(cpuHard),
        memSoftLimitRatio(memSoft),
        memHardLimitRatio(memHard),
        loadUpdatePeriodMs(updatePeriodMs) {}

  // Backward compatibility constructor (defaults to CGROUP_ONLY)
  AdaptiveRateLimiterConfig(
      double cpuSoft,
      double cpuHard,
      double memSoft,
      double memHard,
      uint64_t updatePeriodMs = 100)
      : monitoringMode(ResourceMonitoringMode::CGROUP_ONLY),
        cpuSoftLimitRatio(cpuSoft),
        cpuHardLimitRatio(cpuHard),
        memSoftLimitRatio(memSoft),
        memHardLimitRatio(memHard),
        loadUpdatePeriodMs(updatePeriodMs) {}
};

} // namespace facebook::mononoke
