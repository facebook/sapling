/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <eden/common/utils/WinError.h>
#include <system_error>

namespace facebook::eden {

/**
 * Return true if this exception contains an errno value in ex.code().value()
 */
inline bool isErrnoError(const std::system_error& ex) {
  // std::generic_category is the correct category to represent errno values.
  // However folly/Exception.h unfortunately throws errno values as
  // std::system_category for now.
  return (
      ex.code().category() == std::generic_category() ||
      ex.code().category() == std::system_category());
}

/**
 * Return true if this exception is equivalent to an ENOENT error code.
 */
inline bool isEnoent(const std::system_error& ex) {
  auto ret = isErrnoError(ex) && ex.code().value() == ENOENT;
#ifdef _WIN32
  ret = ret ||
      (ex.code().category() == Win32ErrorCategory::get() &&
       (ex.code().value() == ERROR_PATH_NOT_FOUND ||
        ex.code().value() == ERROR_FILE_NOT_FOUND));
#endif
  return ret;
}

} // namespace facebook::eden
