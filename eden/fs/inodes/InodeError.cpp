/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeError.h"

#include <cerrno>

#include "eden/fs/telemetry/ThrowTraceCapture.h"

#include "eden/fs/inodes/TreeInode.h"

namespace facebook::eden {

namespace {
/**
 * Errors the filesystem raises in normal operation, such as a lookup of a
 * name that does not exist, throw on the request path. Their throw-site
 * trace is never reported and is the most expensive part of the throw.
 */
bool isExpectedInodeError(const void* exception) {
  const auto errnum = static_cast<const InodeError*>(exception)->code().value();
  return errnum == ENOENT || errnum == EEXIST || errnum == ENOTEMPTY;
}

const bool kThrowTracesSkipped = [] {
  skipThrowTraceFor(typeid(InodeError), isExpectedInodeError);
  return true;
}();
} // namespace

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
        auto childName = child_.value().view();
        path.append(childName.begin(), childName.end());
      }
    } else {
      path = inode_->getLogPath();
    }
  }
  return path;
}
} // namespace facebook::eden
