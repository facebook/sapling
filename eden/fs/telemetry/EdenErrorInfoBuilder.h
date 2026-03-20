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

class EdenErrorInfoBuilder {
 public:
  EdenErrorInfoBuilder& withMountPoint(std::string mountPoint);
  EdenErrorInfoBuilder& withInode(uint64_t inode);
  EdenErrorInfoBuilder& withClientCommandName(std::string name);
  EdenErrorInfoBuilder& withErrorCode(int64_t code);
  EdenErrorInfoBuilder& withErrorName(std::string name);
  EdenErrorInfo create();

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
  std::string sourceLocation_;
  std::optional<std::string> clientCommandName_;
  std::optional<uint64_t> inode_;
  std::optional<std::string> mountPoint_;
};

} // namespace facebook::eden
