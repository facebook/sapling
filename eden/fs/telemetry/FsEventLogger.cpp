/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/FsEventLogger.h"

#include <folly/Random.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/telemetry/IHiveLogger.h"

namespace facebook::eden {

namespace {
constexpr size_t kConfigsStringBufferSize = 500;
constexpr auto kConfigsStringRefreshInterval = std::chrono::minutes(30);

std::string getConfigsString(std::shared_ptr<const EdenConfig> config) {
  fmt::memory_buffer buffer;
  // fmt::format_to will grow the buffer if it needs to be longer. However, we
  // should only log what's necessary to not waste logging space.
  buffer.reserve(kConfigsStringBufferSize);

  for (auto& configKey : config->requestSamplingConfigAllowlist.getValue()) {
    try {
      if (auto value = config->getValueByFullKey(configKey)) {
        // e.g.: telemetry:request-samples-per-minute:10;
        fmt::format_to(buffer, "{}:{};", configKey, value.value());
      }
    } catch (const std::exception& ex) {
      XLOG(ERR) << "config key " << configKey
                << " is ill-formed: " << ex.what();
    }
  }

  return fmt::to_string(buffer);
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

  auto configString = configsString_.rlock();
  uint64_t durationUs =
      std::chrono::duration_cast<std::chrono::microseconds>(event.durationNs)
          .count();
  logger_->logFsEventSample({durationUs, event.cause, *configString});
}

} // namespace facebook::eden
