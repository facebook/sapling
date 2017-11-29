/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/FileHandle.h"

#include <folly/experimental/logging/xlog.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

FileHandle::FileHandle(FileInodePtr inode) : inode_(std::move(inode)) {
  inode_->fileHandleDidOpen();
}

FileHandle::~FileHandle() {
  inode_->fileHandleDidClose();
}

folly::Future<fusell::Dispatcher::Attr> FileHandle::getattr() {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "getattr({})",
      inode_->getNodeId());
  return inode_->getattr();
}

fuse_ino_t FileHandle::getInodeNumber() {
  return inode_->getNodeId();
}

folly::Future<fusell::Dispatcher::Attr> FileHandle::setattr(
    const struct stat& attr,
    int to_set) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "setattr({})",
      inode_->getNodeId());
  return inode_->setattr(attr, to_set);
}

bool FileHandle::preserveCache() const {
  return true;
}

bool FileHandle::isSeekable() const {
  return true;
}

folly::Future<fusell::BufVec> FileHandle::read(size_t size, off_t off) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "read({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      size);
  return inode_->read(size, off);
}

folly::Future<size_t> FileHandle::write(fusell::BufVec&& buf, off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->getPath();
    if (myname.hasValue()) {
      inode_->getMount()->getJournal().addDelta(
          std::make_unique<JournalDelta>(JournalDelta{myname.value()}));
    }
  };
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      buf.size());
  return inode_->write(std::move(buf), off);
}

folly::Future<size_t> FileHandle::write(folly::StringPiece str, off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->getPath();
    if (myname.hasValue()) {
      inode_->getMount()->getJournal().addDelta(
          std::make_unique<JournalDelta>(JournalDelta{myname.value()}));
    }
  };
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      inode_->getNodeId(),
      off,
      str.size());
  return inode_->write(str, off);
}

folly::Future<folly::Unit> FileHandle::flush(uint64_t lock_owner) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "flush({})",
      inode_->getNodeId());
  inode_->flush(lock_owner);
  return folly::Unit{};
}

folly::Future<folly::Unit> FileHandle::fsync(bool datasync) {
  FB_LOGF(
      inode_->getMount()->getStraceLogger(),
      DBG7,
      "fsync({})",
      inode_->getNodeId());
  inode_->fsync(datasync);
  return folly::Unit{};
}
} // namespace eden
} // namespace facebook
