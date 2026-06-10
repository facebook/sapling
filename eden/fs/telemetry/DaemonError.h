/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/json/json.h>
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
    if (info.errorType.has_value()) {
      event.addString("error_type", *info.errorType);
    }
    // Sparse per-component fields are serialized into a single "extras"
    // JSON column in Scuba rather than getting their own dedicated columns.
    folly::dynamic extrasObj = folly::dynamic::object;
    if (info.clientCommandName.has_value()) {
      extrasObj["client_command_name"] = *info.clientCommandName;
    }
    if (info.repoName.has_value()) {
      extrasObj["repo_name"] = *info.repoName;
    }
    if (info.fetchType.has_value()) {
      extrasObj["fetch_type"] = *info.fetchType;
    }
    if (info.isDogfoodingHost.has_value()) {
      extrasObj["is_dogfooding_host"] = *info.isDogfoodingHost;
    }
    if (!extrasObj.empty()) {
      event.addString("extras", folly::toJson(extrasObj));
    }
  }
};

} // namespace facebook::eden
