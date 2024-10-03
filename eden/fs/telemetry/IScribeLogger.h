/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/telemetry/SessionInfo.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class EdenConfig;
class EdenMount;

/**
 * A filesystem event to be logged through ScribeLogger.
 * The caller is responsible for ensuring the lifetime of the underlying
 * string exceeds the lifetime of the event.
 */
struct FsEventSample {
  uint64_t durationUs;
  folly::StringPiece cause;
  folly::StringPiece configList;
};

class IScribeLogger {
 public:
  IScribeLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<const EdenConfig> edenConfig)
      : sessionInfo_{std::move(sessionInfo)}, reloadableConfig_{edenConfig} {}
  virtual ~IScribeLogger() = default;

  virtual void log(std::string_view category, std::string&& message) = 0;

  virtual void logFsEventSample(FsEventSample event) = 0;

  /**
   * This allows us to create objects derived from IScribeLogger with
   * only a IScribeLogger pointer
   */
  virtual std::unique_ptr<IScribeLogger> create() = 0;

 protected:
  SessionInfo sessionInfo_;
  ReloadableConfig reloadableConfig_;
};

class NullScribeLogger : public IScribeLogger {
 public:
  NullScribeLogger() : IScribeLogger{SessionInfo{}, {}} {}

  std::unique_ptr<IScribeLogger> create() override {
    return std::make_unique<NullScribeLogger>();
  }

  void log(std::string_view /*category*/, std::string&& /*message*/) override {}

  void logFsEventSample(FsEventSample /* event */) override {}
};

} // namespace facebook::eden
