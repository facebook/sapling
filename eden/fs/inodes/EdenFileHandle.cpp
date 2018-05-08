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

folly::Future<Dispatcher::Attr> EdenFileHandle::getattr() {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "getattr({})",
      inode_->getNodeId());
  return inode_->getattr();
}

InodeNumber EdenFileHandle::getInodeNumber() {
  return inode_->getNodeId();
}

folly::Future<Dispatcher::Attr> EdenFileHandle::setattr(
    const fuse_setattr_in& attr) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "setattr({})",
      inode_->getNodeId());
  return inode_->setattr(attr);
}

bool EdenFileHandle::preserveCache() const {
  return true;
}

bool EdenFileHandle::isSeekable() const {
  return true;
}

folly::Future<BufVec> EdenFileHandle::read(size_t size, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "read({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      size);
  return inode_->read(size, off);
}

folly::Future<size_t> EdenFileHandle::write(BufVec&& buf, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      buf.size());
  return inode_->write(std::move(buf), off).then([inode = inode_](size_t size) {
    auto myname = inode->getPath();
    if (myname.hasValue()) {
      inode->getMount()->getJournal().addDelta(
          std::make_unique<JournalDelta>(JournalDelta{myname.value()}));
    }
    return size;
  });
}

folly::Future<size_t> EdenFileHandle::write(folly::StringPiece str, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      str.size());
  return inode_->write(str, off).then([inode = inode_](size_t size) {
    auto myname = inode->getPath();
    if (myname.hasValue()) {
      inode->getMount()->getJournal().addDelta(
          std::make_unique<JournalDelta>(JournalDelta{myname.value()}));
    }
    return size;
  });
}

folly::Future<folly::Unit> EdenFileHandle::flush(uint64_t lock_owner) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "flush({})",
      inode_->getNodeId());
  inode_->flush(lock_owner);
  return folly::unit;
}

folly::Future<folly::Unit> EdenFileHandle::fsync(bool datasync) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "fsync({})",
      inode_->getNodeId());
  inode_->fsync(datasync);
  return folly::unit;
}
} // namespace eden
} // namespace facebook
