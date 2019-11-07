/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/OverlayFile.h"

#include <folly/FileUtil.h>

#include "eden/fs/inodes/Overlay.h"

namespace facebook {
namespace eden {

OverlayFile::OverlayFile(folly::File file) : file_{std::move(file)} {}

int OverlayFile::fstat(struct stat* buf) const {
  return ::fstat(file_.fd(), buf);
}

ssize_t OverlayFile::preadNoInt(void* buf, size_t n, off_t offset) const {
  return folly::preadNoInt(file_.fd(), buf, n, offset);
}

off_t OverlayFile::lseek(off_t offset, int whence) const {
  return ::lseek(file_.fd(), offset, whence);
}

ssize_t OverlayFile::pwritev(const iovec* iov, int iovcnt, off_t offset) const {
  return folly::pwritevNoInt(file_.fd(), iov, iovcnt, offset);
}

int OverlayFile::ftruncate(off_t length) const {
  return ::ftruncate(file_.fd(), length);
}

int OverlayFile::fsync() const {
  return ::fsync(file_.fd());
}

int OverlayFile::fdatasync() const {
#ifndef __APPLE__
  return ::fdatasync(file_.fd());
#else
  return fsync();
#endif
}

bool OverlayFile::readFile(std::string& out) const {
  return folly::readFile(file_.fd(), out);
}

} // namespace eden
} // namespace facebook
