/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/fscatalog_dev/InodePathDev.h"

namespace facebook::eden {

InodePathDev::InodePathDev() noexcept : path_{'\0'} {}

const char* InodePathDev::c_str() const noexcept {
  return path_.data();
}

InodePathDev::operator RelativePathPiece() const noexcept {
  return RelativePathPiece{folly::StringPiece{c_str()}};
}

std::array<char, InodePathDev::kMaxPathLength>&
InodePathDev::rawData() noexcept {
  return path_;
}

} // namespace facebook::eden

#endif
