/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <chrono>
#include <vector>

#include <folly/Synchronized.h>
#include "folly/Range.h"

namespace facebook::eden {

// `telemetry:request-sampling-group-denominators` should be
// maintained in ascending order so that the higher the sampling group
// the higher the sampling rate.
enum class SamplingGroup : uint32_t {
  DropAll = 0,
  One = 1,
  Two = 2,
  Three = 3,
  Four = 4,
  Five = 5,
};

class ReloadableConfig;
class IHiveLogger;

class FsEventLogger {
 public:
  struct Event {
    std::chrono::nanoseconds durationNs;
    SamplingGroup samplingGroup;
    folly::StringPiece cause;
  };

  FsEventLogger(
      std::shared_ptr<ReloadableConfig> edenConfig,
      std::shared_ptr<IHiveLogger> logger);
  void log(Event event);

 private:
  std::shared_ptr<ReloadableConfig> edenConfig_;
  std::shared_ptr<IHiveLogger> logger_;

  std::atomic<uint32_t> samplesCount_{0};
  std::atomic<std::chrono::steady_clock::time_point> counterStartTime_;

  folly::Synchronized<std::string> configsString_;
  std::atomic<std::chrono::steady_clock::time_point> configsStringUpdateTime_;
};

} // namespace facebook::eden
