/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Format.h>
#include <folly/Range.h>

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
EdenError newEdenError(int errorCode, folly::StringPiece message) {
  auto e = EdenError(message.str());
  e.set_errorCode(errorCode);
  return e;
}

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
template <class... Args>
EdenError newEdenError(const std::system_error& ex) {
  return newEdenError(ex.code().value(), ex.what());
}

/**
 * Construct an EdenError from an exception.
 *
 * If the exception is an instance of std::system_error the error code will be
 * extracted.
 */
template <class... Args>
EdenError newEdenError(const std::exception& ex) {
  const std::system_error* systemError =
      dynamic_cast<const std::system_error*>(&ex);
  if (systemError) {
    return newEdenError(*systemError);
  }
  return EdenError(folly::exceptionStr(ex).toStdString());
}
}
}
