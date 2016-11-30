/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "FileHandle.h"

#include "EdenMount.h"
#include "FileData.h"
#include "FileInode.h"
#include "eden/fs/store/LocalStore.h"


namespace facebook {
namespace eden {

FileHandle::FileHandle(
    std::shared_ptr<FileInode> inode,
    std::shared_ptr<FileData> data,
    int flags)
    : inode_(std::move(inode)), data_(std::move(data)), openFlags_(flags) {}

FileHandle::~FileHandle() {
  // Must reset the data point prior to calling fileHandleDidClose,
  // otherwise it will see a use count that is too high and won't
  // reclaim resources soon enough.
  data_.reset();
  inode_->fileHandleDidClose();
}

folly::Future<fusell::Dispatcher::Attr> FileHandle::getattr() {
  return inode_->getattr();
}

folly::Future<fusell::Dispatcher::Attr> FileHandle::setattr(
    const struct stat& attr,
    int to_set) {
  return inode_->setattr(attr, to_set);
}

bool FileHandle::preserveCache() const {
  return true;
}

bool FileHandle::isSeekable() const {
  return true;
}

folly::Future<fusell::BufVec> FileHandle::read(size_t size, off_t off) {
  return data_->read(size, off);
}

folly::Future<size_t> FileHandle::write(fusell::BufVec&& buf, off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->parentInode_->getNameMgr()->resolvePathToNode(
        inode_->getNodeId());
    inode_->parentInode_->getMount()->getJournal().wlock()->addDelta(
        std::make_unique<JournalDelta>(JournalDelta{myname}));
  };
  return data_->write(std::move(buf), off);
}

folly::Future<size_t> FileHandle::write(folly::StringPiece str, off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->parentInode_->getNameMgr()->resolvePathToNode(
        inode_->getNodeId());
    inode_->parentInode_->getMount()->getJournal().wlock()->addDelta(
        std::make_unique<JournalDelta>(JournalDelta{myname}));
  };
  return data_->write(str, off);
}

folly::Future<folly::Unit> FileHandle::flush(uint64_t lock_owner) {
  data_->flush(lock_owner);
  return folly::Unit{};
}

folly::Future<folly::Unit> FileHandle::fsync(bool datasync) {
  data_->fsync(datasync);
  return folly::Unit{};
}
}
}
