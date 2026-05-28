/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "eden/fs/telemetry/EdenErrorInfo.h"
#include "eden/fs/telemetry/ErrorArg.h"

namespace facebook::eden {

struct DaemonError;

class EdenErrorInfoBuilder {
 public:
  EdenErrorInfoBuilder& withMountPoint(std::string mountPoint);
  EdenErrorInfoBuilder& withInode(std::optional<uint64_t> inode);
  EdenErrorInfoBuilder& withFilePath(std::string filePath);
  EdenErrorInfoBuilder& withErrorCode(int64_t code);
  EdenErrorInfoBuilder& withErrorName(std::string name);
  EdenErrorInfoBuilder& withMountStatus(std::string status);
  EdenErrorInfoBuilder& withErrorType(std::string errorType);
  EdenErrorInfo create();
  DaemonError createEvent();

 private:
  friend class EdenErrorInfo;

  EdenErrorInfoBuilder(
      EdenComponent component,
      const ErrorArg& error,
      SourceInfo loc);

  EdenComponent component_;
  std::string errorMessage_;
  std::optional<int64_t> errorCode_;
  std::optional<std::string> errorName_;
  std::optional<std::string> exceptionType_;
  bool hasCapturedTrace_ = false;
  SourceInfo sourceInfo_;
  std::optional<std::string> clientCommandName_;
  std::optional<uint64_t> inode_;
  std::optional<std::string> filePath_;
  std::optional<std::string> mountPoint_;
  std::optional<std::string> mountStatus_;
  std::optional<std::string> errorType_;
};

} // namespace facebook::eden
