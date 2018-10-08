/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/EdenFileHandle.h"

#include <folly/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

EdenFileHandle::~EdenFileHandle() {
  inode_->fileHandleDidClose();
}

folly::Future<size_t> EdenFileHandle::write(BufVec&& buf, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      buf.size());
  return inode_->write(std::move(buf), off);
}

folly::Future<size_t> EdenFileHandle::write(folly::StringPiece str, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      str.size());
  return inode_->write(str, off);
}
} // namespace eden
} // namespace facebook
