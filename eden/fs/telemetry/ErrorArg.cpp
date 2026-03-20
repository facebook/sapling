/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ErrorArg.h"

#include <system_error>

#include <folly/Demangle.h>

namespace facebook::eden {

ErrorArg::ErrorArg(const std::exception& ex) : message(ex.what()) {
  exceptionType = folly::demangle(typeid(ex)).toStdString();
  if (const auto* sysErr = dynamic_cast<const std::system_error*>(&ex)) {
    errorCode = sysErr->code().value();
    errorName = sysErr->code().message();
  }
}

ErrorArg::ErrorArg(std::string message) : message(std::move(message)) {}

ErrorArg::ErrorArg(const char* message) : message(message) {}

} // namespace facebook::eden
