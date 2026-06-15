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

namespace facebook::eden {

/**
 * Wrapper that can be implicitly constructed from either a std::exception or a
 * std::string. Extracts error message, and optionally error code and error
 * name (from std::system_error).
 */
class ErrorArg {
 public:
  // Implicit conversions — callers can pass either type directly.
  ErrorArg(const std::exception& ex);
  ErrorArg(std::string message);
  ErrorArg(const char* message);

  // Builds an ErrorArg from an exception WITHOUT marking a captured throw-site
  // trace. Use this when the exception is handled away from its throw site
  // (e.g. visited from a folly::Try / exception_wrapper rather than an inline
  // catch), where the thread-local throw-trace does not belong to this
  // exception and would otherwise be misattributed.
  static ErrorArg fromExceptionWithoutTrace(const std::exception& ex);

  std::string message;
  // Numeric errno from std::system_error (e.g. ENOENT=2, EACCES=13).
  std::optional<int64_t> errorCode;
  std::optional<std::string> errorName;
  std::optional<std::string> exceptionType;
  bool hasCapturedTrace = false;
};

} // namespace facebook::eden
