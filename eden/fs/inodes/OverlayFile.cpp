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

folly::Expected<struct stat, int> OverlayFile::fstat() const {
  struct stat st {};
  if (::fstat(file_.fd(), &st)) {
    return folly::makeUnexpected(errno);
  }
  return st;
}

folly::Expected<ssize_t, int>
OverlayFile::preadNoInt(void* buf, size_t n, off_t offset) const {
  auto ret = folly::preadNoInt(file_.fd(), buf, n, offset);
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return ret;
}

folly::Expected<off_t, int> OverlayFile::lseek(off_t offset, int whence) const {
  auto ret = ::lseek(file_.fd(), offset, whence);
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return ret;
}

folly::Expected<ssize_t, int>
OverlayFile::pwritev(const iovec* iov, int iovcnt, off_t offset) const {
  auto ret = folly::pwritevNoInt(file_.fd(), iov, iovcnt, offset);
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return ret;
}

folly::Expected<int, int> OverlayFile::ftruncate(off_t length) const {
  auto ret = ::ftruncate(file_.fd(), length);
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return folly::makeExpected<int>(ret);
}

folly::Expected<int, int> OverlayFile::fsync() const {
  auto ret = ::fsync(file_.fd());
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return folly::makeExpected<int>(ret);
}

folly::Expected<int, int> OverlayFile::fdatasync() const {
#ifndef __APPLE__
  auto ret = ::fdatasync(file_.fd());
  if (ret == -1) {
    return folly::makeUnexpected(errno);
  }
  return folly::makeExpected<int>(ret);
#else
  return fsync();
#endif
}

folly::Expected<std::string, int> OverlayFile::readFile() const {
  std::string out;
  if (!folly::readFile(file_.fd(), out)) {
    return folly::makeUnexpected(errno);
  }
  return folly::makeExpected<int>(std::move(out));
}

} // namespace eden
} // namespace facebook
