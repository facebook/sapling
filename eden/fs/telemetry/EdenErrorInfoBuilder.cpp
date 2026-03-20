/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenErrorInfoBuilder.h"

#include <fmt/core.h>

namespace facebook::eden {

EdenErrorInfoBuilder& EdenErrorInfoBuilder::withMountPoint(
    std::string mountPoint) {
  mountPoint_ = std::move(mountPoint);
  return *this;
}

EdenErrorInfoBuilder& EdenErrorInfoBuilder::withInode(uint64_t inode) {
  inode_ = inode;
  return *this;
}

EdenErrorInfoBuilder& EdenErrorInfoBuilder::withClientCommandName(
    std::string name) {
  clientCommandName_ = std::move(name);
  return *this;
}

EdenErrorInfoBuilder& EdenErrorInfoBuilder::withErrorCode(int64_t code) {
  errorCode_ = code;
  return *this;
}

EdenErrorInfoBuilder& EdenErrorInfoBuilder::withErrorName(std::string name) {
  errorName_ = std::move(name);
  return *this;
}

EdenErrorInfo EdenErrorInfoBuilder::create() {
  EdenErrorInfo info;
  info.component = component_;
  info.errorMessage = std::move(errorMessage_);
  info.errorCode = errorCode_;
  info.errorName = std::move(errorName_);
  info.exceptionType = std::move(exceptionType_);
  // Currently stores source location (file:line in func), will be replaced
  // with a Manifold URL for full stack traces
  info.stackTrace = std::move(sourceLocation_);
  info.clientCommandName = std::move(clientCommandName_);
  info.inode = inode_;
  info.mountPoint = std::move(mountPoint_);
  return info;
}

EdenErrorInfoBuilder::EdenErrorInfoBuilder(
    EdenComponent component,
    const ErrorArg& error,
    SourceInfo loc)
    : component_(component),
      errorMessage_(error.message),
      errorCode_(error.errorCode),
      errorName_(error.errorName),
      exceptionType_(error.exceptionType),
      sourceLocation_(
          fmt::format("{}:{} in {}", loc.file, loc.line, loc.func)) {}

} // namespace facebook::eden
