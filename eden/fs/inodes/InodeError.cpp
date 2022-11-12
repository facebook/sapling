/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "InodeError.h"

#include <folly/String.h>
#include "eden/fs/inodes/TreeInode.h"

namespace facebook::eden {

InodeError::InodeError(int errnum, TreeInodePtr inode, PathComponentPiece child)
    : PathErrorBase(errnum),
      inode_(std::move(inode)),
      child_(PathComponent{child}) {}

InodeError::InodeError(
    int errnum,
    TreeInodePtr inode,
    PathComponentPiece child,
    std::string&& message)
    : PathErrorBase(errnum, std::move(message)),
      inode_(std::move(inode)),
      child_(PathComponent{child}) {}

std::string InodeError::computePath() const noexcept {
  std::string path;
  if (inode_) {
    if (child_.has_value()) {
      if (inode_->getNodeId() == kRootNodeId) {
        path = child_.value().asString();
      } else {
        path = inode_->getLogPath() + "/";
        auto childName = child_.value().stringPiece();
        path.append(childName.begin(), childName.end());
      }
    } else {
      path = inode_->getLogPath();
    }
  }
  return path;
}
} // namespace facebook::eden
