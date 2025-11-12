/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/mononoke/adaptive_rate_limiter/cpp/AdaptiveRateLimiterConfig.h"

// Forward declarations to avoid including heavy proxygen headers
namespace proxygen {
class BaseAdaptiveRateLimiter;
class ARLResourceStats;
class AdaptiveRateLimiterConfiguration;
} // namespace proxygen

namespace facebook::mononoke {

/**
 * Wrapper around proxygen's AdaptiveRateLimiter for use in Rust services.
 *
 * This class provides a simplified interface to the underlying C++ ARL
 * implementation, hiding the complexity of resource monitoring and
 * configuration management.
 *
 * Usage:
 *   auto config = AdaptiveRateLimiterConfig(0.7, 0.85, 0.8, 0.95);
 *   auto limiter = AdaptiveRateLimiterWrapper::create(config);
 *   if (limiter->shouldShed()) {
 *     // Reject request
 *   }
 */
class AdaptiveRateLimiterWrapper {
 public:
  /**
   * Constructor to create a new rate limiter instance.
   *
   * @param config Configuration with CPU/memory thresholds
   */
  explicit AdaptiveRateLimiterWrapper(const AdaptiveRateLimiterConfig& config);

  /**
   * Factory method to create a new rate limiter instance.
   * Provided for convenience and consistency with other API patterns.
   *
   * @param config Configuration with CPU/memory thresholds
   * @return Unique pointer to the rate limiter wrapper
   */
  static std::unique_ptr<AdaptiveRateLimiterWrapper> create(
      const AdaptiveRateLimiterConfig& config);

  ~AdaptiveRateLimiterWrapper();

  /**
   * Check if the current request should be shed (rejected) based on
   * system resource utilization (CPU and memory).
   *
   * This method:
   * 1. Reads current CPU and memory usage from cgroup/host
   * 2. Computes saturation ratios based on configured thresholds
   * 3. Returns true if request should be shed (probabilistic)
   *
   * Thread-safe: Can be called from multiple threads concurrently
   *
   * @return true if request should be shed, false otherwise
   */
  bool shouldShed();

  /**
   * Update configuration at runtime.
   *
   * @param config New configuration to apply
   */
  void updateConfig(const AdaptiveRateLimiterConfig& config);

 private:
  // Initialize the underlying ARL components
  void initialize(const AdaptiveRateLimiterConfig& config);

  // Proxygen ARL components (opaque pointers to avoid header dependencies)
  std::unique_ptr<proxygen::BaseAdaptiveRateLimiter> rateLimiter_;
  std::unique_ptr<proxygen::ARLResourceStats> resourceStats_;
  std::unique_ptr<proxygen::ARLResourceStats>
      hostResourceStats_; // For BOTH mode
  std::shared_ptr<proxygen::AdaptiveRateLimiterConfiguration> arlConfig_;

  // Track current monitoring mode for updateConfig
  ResourceMonitoringMode currentMonitoringMode_{
      ResourceMonitoringMode::CGROUP_ONLY};
};

} // namespace facebook::mononoke
