/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/ranges.h>
#include <folly/ExceptionWrapper.h>
#include <string_view>
#include <system_error>
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/Utf8.h"

namespace facebook::eden {
/*
 * Helper functions for constructing thrift EdenError objects.
 */

/**
 * Construct an EdenError from an error code, error type, and message.
 *
 * The message arguments will be concatenated, each formatted with fmt.
 */
template <class Arg1, class... Args>
EdenError newEdenError(
    int errorCode,
    EdenErrorType errorType,
    const Arg1& msg,
    const Args&... args) {
  auto e = EdenError{ensureValidUtf8(fmt::to_string(fmt::join(
      std::make_tuple<const Arg1&, const Args&...>(msg, args...), "")))};
  e.errorCode_ref() = errorCode;
  e.errorType_ref() = errorType;
  return e;
}

/**
 * Construct an EdenError with an error message and error type but no error
 * code.
 *
 * The message arguments will be concatenated, each formatted with fmt.
 *
 * The first message argument must be a string, primarily to help distinguish
 * this version of newEdenError() from the one above that takes an error code as
 * the first argument.  (This is just to eliminate confusion if called with a
 * numeric type other than `int` as the first argument.)
 */
template <class... Args>
EdenError newEdenError(
    EdenErrorType errorType,
    std::string_view msg,
    const Args&... args) {
  auto e = EdenError{ensureValidUtf8(fmt::to_string(fmt::join(
      std::make_tuple<const std::string_view&, const Args&...>(msg, args...),
      "")))};
  e.errorType_ref() = errorType;
  return e;
}

/**
 * Construct an EdenError from a std::system_error.
 *
 * This automatically extracts the error code.
 */
EdenError newEdenError(const std::system_error& ex);

/**
 * Construct an EdenError from an exception.
 *
 * If the exception is an instance of std::system_error the error code will be
 * extracted.
 */
EdenError newEdenError(const std::exception& ex);

/**
 * Construct an EdenError from a folly::exception_wrapper.
 */
EdenError newEdenError(const folly::exception_wrapper& ew);
} // namespace facebook::eden
