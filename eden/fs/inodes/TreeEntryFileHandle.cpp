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

#include "TreeEntryFileInode.h"
#include "eden/fs/store/LocalStore.h"

#include <folly/io/Cursor.h>

namespace facebook {
namespace eden {

TreeEntryFileHandle::TreeEntryFileHandle(
    std::shared_ptr<TreeEntryFileInode> inode)
    : inode_(inode) {}

TreeEntryFileHandle::~TreeEntryFileHandle() {
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
  std::unique_lock<std::mutex> lock(inode_->mutex_);

  auto buf = inode_->blob_->getContents();
  folly::io::Cursor cursor(&buf);

  if (!cursor.canAdvance(off)) {
    // Seek beyond EOF.  Return an empty result.
    return fusell::BufVec(folly::IOBuf::wrapBuffer("", 0));
  }

  cursor.skip(off);

  std::unique_ptr<folly::IOBuf> result;
  cursor.cloneAtMost(result, size);
  return fusell::BufVec(std::move(result));
}

folly::Future<size_t> TreeEntryFileHandle::write(fusell::BufVec&&, off_t) {
  // man 2 write: EBADF  fd is not open for writing.
  folly::throwSystemErrorExplicit(EBADF);
}

folly::Future<size_t> TreeEntryFileHandle::write(folly::StringPiece, off_t) {
  // man 2 write: EBADF  fd is not open for writing.
  folly::throwSystemErrorExplicit(EBADF);
}

folly::Future<folly::Unit> TreeEntryFileHandle::flush(uint64_t) {
  // We're read only, so there is nothing to flush.
  return folly::Unit{};
}

folly::Future<folly::Unit> TreeEntryFileHandle::fsync(bool) {
  // We're read only, so there is nothing to sync.
  return folly::Unit{};
}
}
}
