/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeEntryFileHandle.h"

#include "EdenMount.h"
#include "FileData.h"
#include "TreeEntryFileInode.h"
#include "eden/fs/store/LocalStore.h"


namespace facebook {
namespace eden {

TreeEntryFileHandle::TreeEntryFileHandle(
    std::shared_ptr<TreeEntryFileInode> inode,
    std::shared_ptr<FileData> data,
    int flags)
    : inode_(inode), data_(data), openFlags_(flags) {}

TreeEntryFileHandle::~TreeEntryFileHandle() {
  // Must reset the data point prior to calling fileHandleDidClose,
  // otherwise it will see a use count that is too high and won't
  // reclaim resources soon enough.
  data_.reset();
  inode_->fileHandleDidClose();
}

folly::Future<fusell::Dispatcher::Attr> TreeEntryFileHandle::getattr() {
  return inode_->getattr();
}

folly::Future<fusell::Dispatcher::Attr> TreeEntryFileHandle::setattr(
    const struct stat& attr,
    int to_set) {
  return inode_->setattr(attr, to_set);
}

bool TreeEntryFileHandle::preserveCache() const {
  return true;
}

bool TreeEntryFileHandle::isSeekable() const {
  return true;
}

folly::Future<fusell::BufVec> TreeEntryFileHandle::read(
    size_t size,
    off_t off) {
  return data_->read(size, off);
}

folly::Future<size_t> TreeEntryFileHandle::write(
    fusell::BufVec&& buf,
    off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->parentInode_->getNameMgr()->resolvePathToNode(
        inode_->getNodeId());
    inode_->parentInode_->getMount()->getJournal().wlock()->addDelta(
        std::make_unique<JournalDelta>(JournalDelta{myname}));
  };
  return data_->write(std::move(buf), off);
}

folly::Future<size_t> TreeEntryFileHandle::write(
    folly::StringPiece str,
    off_t off) {
  SCOPE_SUCCESS {
    auto myname = inode_->parentInode_->getNameMgr()->resolvePathToNode(
        inode_->getNodeId());
    inode_->parentInode_->getMount()->getJournal().wlock()->addDelta(
        std::make_unique<JournalDelta>(JournalDelta{myname}));
  };
  return data_->write(str, off);
}

folly::Future<folly::Unit> TreeEntryFileHandle::flush(uint64_t lock_owner) {
  data_->flush(lock_owner);
  return folly::Unit{};
}

folly::Future<folly::Unit> TreeEntryFileHandle::fsync(bool datasync) {
  data_->fsync(datasync);
  return folly::Unit{};
}
}
}
