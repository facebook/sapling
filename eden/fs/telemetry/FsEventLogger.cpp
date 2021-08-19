/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/FsEventLogger.h"

#include <folly/Random.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"

namespace facebook::eden {

namespace {
constexpr auto kConfigsStringRefreshInterval = std::chrono::minutes(30);

std::string getConfigsString(std::shared_ptr<const EdenConfig>) {
  // TODO: get config string
  return {};
}

} // namespace

FsEventLogger::FsEventLogger(
    ReloadableConfig& edenConfig,
    std::shared_ptr<IHiveLogger> logger)
    : edenConfig_{edenConfig},
      logger_{std::move(logger)},
      counterStartTime_{std::chrono::steady_clock::now()},
      configsString_{getConfigsString(edenConfig_.getEdenConfig())},
      configsStringUpdateTime_{std::chrono::steady_clock::now()} {}

void FsEventLogger::log(Event event) {
  if (event.samplingGroup == SamplingGroup::DropAll) {
    return;
  }

  auto config = edenConfig_.getEdenConfig(ConfigReloadBehavior::NoReload);

  const auto& denominators =
      config->requestSamplingGroupDenominators.getValue();
  auto samplingGroup = folly::to_underlying(event.samplingGroup);
  if (samplingGroup > denominators.size()) {
    // sampling group does not exist
    return;
  }
  if (auto sampleDenominator = denominators.at(samplingGroup);
      sampleDenominator && 0 != folly::Random::rand32(sampleDenominator)) {
    // failed sampling
    return;
  }

  // Multiple threads could enter the branches at the same time
  // resulting in samplesCount_ undercounting, but this should rarely happen
  // given the sampling above.
  auto now = std::chrono::steady_clock::now();
  if ((now - counterStartTime_.load()) > std::chrono::minutes(1)) {
    // reset counter for this minute
    counterStartTime_.store(now);
    samplesCount_.store(1);
  } else if (
      samplesCount_.load() < config->requestSamplesPerMinute.getValue()) {
    // not throttled so bump counter
    samplesCount_.fetch_add(1, std::memory_order_relaxed);
  } else {
    // throttled
    return;
  }

  if ((now - configsStringUpdateTime_.load()) > kConfigsStringRefreshInterval) {
    configsStringUpdateTime_.store(now);
    *configsString_.wlock() = getConfigsString(edenConfig_.getEdenConfig());
  }

  // TODO: log
}

} // namespace facebook::eden
