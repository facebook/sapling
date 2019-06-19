/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "InodeError.h"

#include <folly/String.h>
#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {

InodeError::InodeError(int errnum, TreeInodePtr inode, PathComponentPiece child)
    : std::system_error(errnum, std::generic_category()),
      inode_(std::move(inode)),
      child_(PathComponent{child}) {}

InodeError::InodeError(
    int errnum,
    TreeInodePtr inode,
    PathComponentPiece child,
    std::string&& message)
    : std::system_error(errnum, std::generic_category()),
      inode_(std::move(inode)),
      child_(PathComponent{child}),
      message_(std::move(message)) {}

const char* InodeError::what() const noexcept {
  try {
    auto msg = fullMessage_.wlock();
    if (msg->empty()) {
      *msg = computeMessage();
    }

    return msg->c_str();
  } catch (...) {
    // Fallback value if anything goes wrong building the real message
    return "<InodeError>";
  }
}

std::string InodeError::computeMessage() const {
  std::string path;
  if (child_.has_value()) {
    if (inode_->getNodeId() == kRootNodeId) {
      path = child_.value().stringPiece().str();
    } else {
      path = inode_->getLogPath() + "/";
      auto childName = child_.value().stringPiece();
      path.append(childName.begin(), childName.end());
    }
  } else {
    path = inode_->getLogPath();
  }

  if (message_.empty()) {
    return path + ": " + folly::errnoStr(errnum()).toStdString();
  }
  return path + ": " + message_ + ": " +
      folly::errnoStr(errnum()).toStdString();
}
} // namespace eden
} // namespace facebook
