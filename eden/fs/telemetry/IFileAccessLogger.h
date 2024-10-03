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

struct FileAccess {
  InodeNumber inodeNumber;
  ObjectFetchContext::Cause cause;
  std::optional<std::string> causeDetail;
  std::weak_ptr<EdenMount> edenMount;
};

class IFileAccessLogger {
 public:
  IFileAccessLogger(
      SessionInfo sessionInfo,
      std::shared_ptr<const EdenConfig> edenConfig)
      : sessionInfo_{std::move(sessionInfo)}, reloadableConfig_{edenConfig} {}
  virtual ~IFileAccessLogger() = default;

  virtual void logFileAccess(FileAccess access) = 0;

  /**
   * This allows us to create objects derived from IFileAccessLogger with
   * only a IFileAccessLogger pointer
   */
  virtual std::unique_ptr<IFileAccessLogger> create() = 0;

 protected:
  SessionInfo sessionInfo_;
  ReloadableConfig reloadableConfig_;
};

class NullFileAccessLogger : public IFileAccessLogger {
 public:
  NullFileAccessLogger() : IFileAccessLogger{SessionInfo{}, {}} {}

  std::unique_ptr<IFileAccessLogger> create() override {
    return std::make_unique<NullFileAccessLogger>();
  }

  void logFileAccess(FileAccess /* access */) override {}
};

} // namespace facebook::eden
