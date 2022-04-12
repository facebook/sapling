/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/PathError.h"

#include <folly/String.h>

namespace facebook::eden {

const char* PathErrorBase::what() const noexcept {
  try {
    auto msg = fullMessage_.wlock();
    if (msg->empty()) {
      *msg = computeMessage();
    }

    return msg->c_str();
  } catch (...) {
    // Fallback value if anything goes wrong building the real message
    return "<PathErrorBase>";
  }
}

std::string PathErrorBase::computeMessage() const {
  std::string path = computePath().c_str();
  if (message_.empty()) {
    return path + ": " + folly::errnoStr(code().value());
  }
  return path + ": " + message_ + ": " + folly::errnoStr(code().value());
}
} // namespace facebook::eden
