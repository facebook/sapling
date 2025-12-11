/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/mononoke/common/adaptive_rate_limiter/cpp/AdaptiveRateLimiterWrapper.h"

#include <chrono>
#include <memory>
#include <string>

#include <folly/Memory.h>
#include <folly/logging/xlog.h>

#include "proxygen/facebook/lib/services/ARLLinearShedAlgo.h"
#include "proxygen/facebook/lib/services/AdaptiveRateLimiter.h"
#include "proxygen/facebook/lib/services/MultiLevelAdaptiveRateLimiter.h"
#include "proxygen/facebook/lib/services/SimpleMultiLevelShedAlgo.h"
#include "proxygen/facebook/lib/statistics/ARLResourceStats.h"
#include "proxygen/facebook/lib/statistics/CgroupResources.h"
#include "proxygen/facebook/lib/statistics/HostResources.h"

namespace facebook::mononoke {

AdaptiveRateLimiterWrapper::AdaptiveRateLimiterWrapper(
    const AdaptiveRateLimiterConfig& config) {
  initialize(config);
}

AdaptiveRateLimiterWrapper::~AdaptiveRateLimiterWrapper() = default;

std::unique_ptr<AdaptiveRateLimiterWrapper> AdaptiveRateLimiterWrapper::create(
    const AdaptiveRateLimiterConfig& config) {
  return std::make_unique<AdaptiveRateLimiterWrapper>(config);
}

void AdaptiveRateLimiterWrapper::initialize(
    const AdaptiveRateLimiterConfig& config) {
  // Track operation mode
  currentOperationMode_ = config.operationMode;

  // If monitoring is disabled, skip initialization
  if (config.monitoringMode == ResourceMonitoringMode::NONE) {
    return;
  }

  // Create resource monitoring based on mode
  switch (config.monitoringMode) {
    case ResourceMonitoringMode::CGROUP_ONLY:
      resourceStats_ = std::make_unique<proxygen::ARLResourceStats>(
          std::make_unique<proxygen::CgroupResources>());
      break;

    case ResourceMonitoringMode::HOST_ONLY:
      resourceStats_ = std::make_unique<proxygen::ARLResourceStats>(
          std::make_unique<proxygen::HostResources>());
      break;

    case ResourceMonitoringMode::BOTH:
      // For BOTH mode, we use MultiLevelAdaptiveRateLimiter
      // which requires separate host and cgroup resource stats
      // We'll initialize these in the BOTH case below
      resourceStats_ = std::make_unique<proxygen::ARLResourceStats>(
          std::make_unique<proxygen::CgroupResources>());
      hostResourceStats_ = std::make_unique<proxygen::ARLResourceStats>(
          std::make_unique<proxygen::HostResources>());
      break;

    case ResourceMonitoringMode::NONE:
      // Already handled above
      return;
  }

  // Create the ARL configuration
  arlConfig_ = std::make_shared<proxygen::AdaptiveRateLimiterConfiguration>();

  // Configure CPU thresholds
  arlConfig_->setCpuSoftLimitRatio(config.cpuSoftLimitRatio);
  arlConfig_->setCpuHardLimitRatio(config.cpuHardLimitRatio);

  // Configure memory thresholds
  arlConfig_->setMemSoftLimitRatio(config.memSoftLimitRatio);
  arlConfig_->setMemHardLimitRatio(config.memHardLimitRatio);

  // Enable load shedding
  arlConfig_->setLoadSheddingEnabled(true);

  // Enable request-level shedding (not connection-level)
  arlConfig_->setReqModToggle(true);
  arlConfig_->setConnModToggle(false);

  // Set the load update period
  arlConfig_->setLoadUpdatePeriod(
      std::chrono::milliseconds(config.loadUpdatePeriodMs));

  // Pass config to resource stats for monitoring
  resourceStats_->setARLConfig(arlConfig_);
  if (hostResourceStats_) {
    hostResourceStats_->setARLConfig(arlConfig_);
  }

  // Create the appropriate shedding algorithm and rate limiter
  if (config.monitoringMode == ResourceMonitoringMode::BOTH) {
    // Multi-level shedding (host + cgroup)
    auto multiLevelShedAlgo =
        std::make_shared<proxygen::SimpleMultiLevelShedAlgo>();

    auto multiLevelLimiter =
        std::make_unique<proxygen::MultiLevelAdaptiveRateLimiter>();
    multiLevelLimiter->setShedAlgo(multiLevelShedAlgo);
    multiLevelLimiter->setGlobalResourceStats(hostResourceStats_.get());
    multiLevelLimiter->setLocalResourceStats(resourceStats_.get());

    rateLimiter_ = std::move(multiLevelLimiter);
  } else {
    // Single-level shedding (cgroup or host only)
    auto shedAlgo = std::make_shared<proxygen::ARLLinearShedAlgo>();

    auto singleLevelLimiter = std::make_unique<proxygen::AdaptiveRateLimiter>();
    singleLevelLimiter->setShedAlgo(shedAlgo).setResourceStats(
        resourceStats_.get());

    rateLimiter_ = std::move(singleLevelLimiter);
  }
}

bool AdaptiveRateLimiterWrapper::shouldShed() {
  // If operation mode is DISABLED, never shed
  if (currentOperationMode_ == OperationMode::DISABLED) {
    return false;
  }

  // If limiter is not initialized (NONE mode), never shed
  if (!rateLimiter_) {
    return false;
  }

  // Create a dummy request context
  // Since we don't have actual connection/request details in this
  // simplified interface, we use default values
  proxygen::BaseAdaptiveRateLimiter::RequestContext requestContext(
      proxygen::BaseAdaptiveRateLimiter::TransportProtocol::TCP,
      folly::SocketAddress(), // Empty address
      0); // Request sequence number

  // Ask ARL if we should shed this request
  bool shouldShedRequest =
      rateLimiter_->shouldShedReq(*arlConfig_, requestContext);

  // Handle DRY_RUN mode: log but never actually shed
  if (currentOperationMode_ == OperationMode::DRY_RUN) {
    if (shouldShedRequest) {
      logSheddingReason();
    }
    return false;
  }

  // ENABLED mode: log and shed
  if (shouldShedRequest) {
    logSheddingReason();
  }

  return shouldShedRequest;
}

void AdaptiveRateLimiterWrapper::logSheddingReason() {
  if (!resourceStats_) {
    return;
  }

  auto& stats = resourceStats_->getCurrentData();
  std::string resourceType;

  // Determine resource type based on monitoring mode
  switch (currentMonitoringMode_) {
    case ResourceMonitoringMode::CGROUP_ONLY:
      resourceType = "CGROUP";
      break;
    case ResourceMonitoringMode::HOST_ONLY:
      resourceType = "HOST";
      break;
    case ResourceMonitoringMode::BOTH:
      resourceType = "BOTH(CGROUP+HOST)";
      break;
    case ResourceMonitoringMode::NONE:
      resourceType = "NONE";
      break;
  }

  std::string message;
  message += " ResourceType=";
  message += resourceType;
  message += " CPU=";
  message += std::to_string(stats.getCpuPctUtil());
  message += " CPUSoftLimit=";
  message += std::to_string(arlConfig_->getCpuSoftLimitRatio());
  message += " CPUHardLimit=";
  message += std::to_string(arlConfig_->getCpuHardLimitRatio());
  message += " MEM=";
  message += std::to_string(stats.getUsedMemPct());
  message += " MEMSoftLimit=";
  message += std::to_string(arlConfig_->getMemSoftLimitRatio());
  message += " MEMHardLimit=";
  message += std::to_string(arlConfig_->getMemHardLimitRatio());

  // Add operation mode to the message
  std::string opMode;
  switch (currentOperationMode_) {
    case OperationMode::DISABLED:
      opMode = "DISABLED";
      break;
    case OperationMode::ENABLED:
      opMode = "ENABLED";
      break;
    case OperationMode::DRY_RUN:
      opMode = "DRY_RUN";
      break;
  }
  message += " OperationMode=";
  message += opMode;

  // Log at most once per second
  XLOG_EVERY_MS(WARN, 1000)
      << "AdaptiveRateLimiter shedding request:" << message;
}

void AdaptiveRateLimiterWrapper::updateConfig(
    const AdaptiveRateLimiterConfig& config) {
  // Track operation mode
  currentOperationMode_ = config.operationMode;

  // If monitoring mode changed, reinitialize
  // (This is a simple approach; more sophisticated would update in-place)
  if (!rateLimiter_ || config.monitoringMode != currentMonitoringMode_) {
    initialize(config);
    currentMonitoringMode_ = config.monitoringMode;
    return;
  }

  // Update CPU thresholds
  arlConfig_->setCpuSoftLimitRatio(config.cpuSoftLimitRatio);
  arlConfig_->setCpuHardLimitRatio(config.cpuHardLimitRatio);

  // Update memory thresholds
  arlConfig_->setMemSoftLimitRatio(config.memSoftLimitRatio);
  arlConfig_->setMemHardLimitRatio(config.memHardLimitRatio);

  // Update load period
  arlConfig_->setLoadUpdatePeriod(
      std::chrono::milliseconds(config.loadUpdatePeriodMs));

  // Update resource stats with new config
  resourceStats_->setARLConfig(arlConfig_);
  if (hostResourceStats_) {
    hostResourceStats_->setARLConfig(arlConfig_);
  }
}

} // namespace facebook::mononoke
