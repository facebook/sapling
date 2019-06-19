/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/ExceptionWrapper.h>
#include <folly/Format.h>
#include <folly/Range.h>
#include <system_error>
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook {
namespace eden {

/*
 * Helper functions for constructing thrift EdenError objects.
 */

/**
 * Construct an EdenError from an error code and message.
 *
 * The message is used literally in this case, and is not passed through
 * folly::format() if no format arguments are supplied..
 */
EdenError newEdenError(int errorCode, folly::StringPiece message);

/**
 * Construct an EdenError from an error code, plus a format
 * string and format arguments.
 */
template <class... Args>
EdenError newEdenError(int errorCode, folly::StringPiece fmt, Args&&... args) {
  auto e = EdenError(folly::sformat(fmt, std::forward<Args>(args)...));
  e.set_errorCode(errorCode);
  return e;
}

/**
 * Construct an EdenError from a format string and format arguments, with no
 * error code.
 */
template <class... Args>
EdenError newEdenError(folly::StringPiece fmt, Args&&... args) {
  return EdenError(folly::sformat(fmt, std::forward<Args>(args)...));
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
