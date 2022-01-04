/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class EdenConfig;
class EdenMount;

struct FileAccess {
  InodeNumber inodeNumber;
  ObjectFetchContext::Cause cause;
  std::optional<std::string> causeDetail;
  std::weak_ptr<EdenMount> edenMount;
};

/**
 * A filesystem event to be logged through HiveLogger.
 * The caller is responsible for ensuring the lifetime of the underlying
 * string exceeds the lifetime of the event.
 */
struct FsEventSample {
  uint64_t durationUs;
  folly::StringPiece cause;
  folly::StringPiece configList;
};

// TODO: Deprecate ScribeLogger and rename this class ScribeLogger.
class IHiveLogger {
 public:
  IHiveLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<const EdenConfig> edenConfig)
      : sessionInfo_{std::move(sessionInfo)}, reloadableConfig_{edenConfig} {}
  virtual ~IHiveLogger() = default;

  virtual void log(std::string_view category, std::string&& message) = 0;

  virtual void logFileAccess(FileAccess access) = 0;

  virtual void logFsEventSample(FsEventSample event) = 0;

  /**
   * This allows us to create objects derived from IHiveLogger with
   * only a IHiveLogger pointer
   */
  virtual std::unique_ptr<IHiveLogger> create() = 0;

 protected:
  SessionInfo sessionInfo_;
  ReloadableConfig reloadableConfig_;
};

class NullHiveLogger : public IHiveLogger {
 public:
  NullHiveLogger() : IHiveLogger{SessionInfo{}, {}} {}

  std::unique_ptr<IHiveLogger> create() override {
    return std::make_unique<NullHiveLogger>();
  }

  void log(std::string_view /*category*/, std::string&& /*message*/) override {}

  void logFileAccess(FileAccess /* access */) override {}

  void logFsEventSample(FsEventSample /* event */) override {}
};

} // namespace facebook::eden
