/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <system_error>

namespace facebook {
namespace eden {

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
  return isErrnoError(ex) && ex.code().value() == ENOENT;
}

} // namespace eden
} // namespace facebook
