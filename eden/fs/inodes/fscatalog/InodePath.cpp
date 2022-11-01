/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/fscatalog/InodePath.h"

namespace facebook::eden {

InodePath::InodePath() noexcept : path_{'\0'} {}

const char* InodePath::c_str() const noexcept {
  return path_.data();
}

InodePath::operator RelativePathPiece() const noexcept {
  return RelativePathPiece{folly::StringPiece{c_str()}};
}

std::array<char, InodePath::kMaxPathLength>& InodePath::rawData() noexcept {
  return path_;
}

} // namespace facebook::eden

#endif
