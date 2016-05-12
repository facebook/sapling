/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MercurialManifestFileHandle.h"

namespace facebook {
namespace eden {

MercurialManifestFileHandle::MercurialManifestFileHandle(
    std::shared_ptr<fusell::InodeBase> inode,
    std::string&& content)
    : inode_(inode), content_(std::move(content)) {}

folly::Future<fusell::Dispatcher::Attr> MercurialManifestFileHandle::getattr() {
  return inode_->getattr();
}

folly::Future<fusell::Dispatcher::Attr> MercurialManifestFileHandle::setattr(
    const struct stat& attr,
    int to_set) {
  return inode_->setattr(attr, to_set);
}

bool MercurialManifestFileHandle::preserveCache() const {
  return true;
}

bool MercurialManifestFileHandle::isSeekable() const {
  return true;
}

folly::Future<fusell::BufVec> MercurialManifestFileHandle::read(
    size_t size,
    off_t off) {
  folly::ByteRange toread((uint8_t*)content_.data(), content_.size());
  // Clamp to a reasonable region
  if (off <= toread.size()) {
    toread.advance(off);
  }
  size = std::min(toread.size(), size);
  toread.subtract(toread.size() - size);
  // No copy made; FUSE guarantees that our FileHandle instance outlives
  // the BufVec and associated IOBuf
  return fusell::BufVec(folly::IOBuf::wrapBuffer(toread));
}

folly::Future<size_t> MercurialManifestFileHandle::write(
    fusell::BufVec&&,
    off_t) {
  folly::throwSystemErrorExplicit(EBADF);
}

folly::Future<size_t> MercurialManifestFileHandle::write(
    folly::StringPiece,
    off_t) {
  folly::throwSystemErrorExplicit(EBADF);
}

folly::Future<folly::Unit> MercurialManifestFileHandle::flush(uint64_t) {
  return folly::Unit{};
}

folly::Future<folly::Unit> MercurialManifestFileHandle::fsync(bool) {
  return folly::Unit{};
}
}
}
