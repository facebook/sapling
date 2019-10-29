/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Conv.h>
#include <folly/ExceptionWrapper.h>
#include <folly/Range.h>
#include <system_error>
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook {
namespace eden {
/*
 * Helper functions for constructing thrift EdenError objects.
 */

/**
 * Construct an EdenError from an error code, error type, and message.
 *
 * The message arguments will be joined with folly::to<std::string>().
 */
template <class Arg1, class... Args>
EdenError newEdenError(
    int errorCode,
    EdenErrorType errorType,
    Arg1&& msg,
    Args&&... args) {
  auto e = EdenError(folly::to<std::string>(
      std::forward<Arg1>(msg), std::forward<Args>(args)...));
  e.set_errorCode(errorCode);
  e.set_errorType(errorType);
  return e;
}

/**
 * Construct an EdenError with an error message and error type but no error
 * code.
 *
 * The message arguments will be joined with folly::to<std::string>().
 *
 * The first message argument must be a string, primarily to help distinguish
 * this version of newEdenError() from the one above that takes an error code as
 * the first argument.  (This is just to eliminate confusion if called with a
 * numeric type other than `int` as the first argument.)
 */
template <class... Args>
EdenError
newEdenError(EdenErrorType errorType, folly::StringPiece msg, Args&&... args) {
  auto e = EdenError(folly::to<std::string>(msg, std::forward<Args>(args)...));
  e.set_errorType(errorType);
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
} // namespace eden
} // namespace facebook
