/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/telemetry/LogEvent.h"
#include "eden/fs/telemetry/EdenErrorInfo.h"

namespace facebook::eden {

struct DaemonError : public TypelessEvent {
  EdenErrorInfo info;

  explicit DaemonError(EdenErrorInfo info) : info(std::move(info)) {}

  void populate(DynamicEvent& event) const override {
    event.addString("component", std::string(toString(info.component)));
    event.addString("error_message", info.errorMessage);
    if (info.exceptionType.has_value()) {
      event.addString("exception_type", *info.exceptionType);
    }
    if (info.errorCode.has_value()) {
      event.addInt("error_code", *info.errorCode);
    }
    if (info.errorName.has_value()) {
      event.addString("error_name", *info.errorName);
    }
    if (info.stackTrace.has_value()) {
      event.addString("stack_trace", *info.stackTrace);
    }
    if (info.clientCommandName.has_value()) {
      event.addString("client_command_name", *info.clientCommandName);
    }
    if (info.inode.has_value()) {
      event.addInt("inode", static_cast<int64_t>(*info.inode));
    }
    if (info.filePath.has_value()) {
      event.addString("file_path", *info.filePath);
    }
    if (info.mountPoint.has_value()) {
      event.addString("mount_point", *info.mountPoint);
    }
    if (info.mountStatus.has_value()) {
      event.addString("mount_status", *info.mountStatus);
    }
  }
};

} // namespace facebook::eden
