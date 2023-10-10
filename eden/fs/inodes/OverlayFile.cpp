/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/OverlayFile.h"

#include <folly/FileUtil.h>

#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/utils/NotImplemented.h"

namespace facebook::eden {

OverlayFile::OverlayFile(folly::File file, std::weak_ptr<Overlay> overlay)
    : data_{std::move(file)}, overlay_{overlay} {}

OverlayFile::OverlayFile(InodeNumber ino, std::weak_ptr<Overlay> overlay)
    : data_{ino}, overlay_{overlay} {}

OverlayFile::OverlayFile(
    std::variant<folly::File, InodeNumber> data,
    std::weak_ptr<Overlay> overlay)
    : data_{std::move(data)}, overlay_{overlay} {}

folly::Expected<struct stat, int> OverlayFile::fstat() const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  IORequest req{overlay.get()};

  struct stat st {};
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    if (::fstat(file.fd(), &st)) {
      return folly::makeUnexpected(errno);
    }
    return st;
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<ssize_t, int>
OverlayFile::preadNoInt(void* buf, size_t n, FileOffset offset) const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }

  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};
    auto ret = folly::preadNoInt(file.fd(), buf, n, offset);
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return ret;
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<FileOffset, int> OverlayFile::lseek(
    FileOffset offset,
    int whence) const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }

  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    auto ret = ::lseek(file.fd(), offset, whence);
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return ret;
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<ssize_t, int>
OverlayFile::pwritev(const iovec* iov, int iovcnt, FileOffset offset) const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    auto ret = folly::pwritevNoInt(file.fd(), iov, iovcnt, offset);
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return ret;
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<int, int> OverlayFile::ftruncate(FileOffset length) const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    auto ret = ::ftruncate(file.fd(), length);
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return folly::makeExpected<int>(ret);
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<int, int> OverlayFile::fsync() const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    auto ret = ::fsync(file.fd());
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return folly::makeExpected<int>(ret);
  } else {
    NOT_IMPLEMENTED();
  }
}

folly::Expected<int, int> OverlayFile::fallocate(
    FileOffset offset,
    FileOffset length) const {
#ifdef __linux__
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    // Don't use posix_fallocate, because glibc may try to emulate it with
    // writes to each chunk, and we definitely don't want that.
    auto ret = ::fallocate(file.fd(), 0, offset, length);
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return folly::makeExpected<int>(ret);
  } else {
    NOT_IMPLEMENTED();
  }
#else
  (void)offset;
  (void)length;
  return folly::makeUnexpected(ENOSYS);
#endif
}

folly::Expected<int, int> OverlayFile::fdatasync() const {
#ifndef __APPLE__
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    auto ret = ::fdatasync(file.fd());
    if (ret == -1) {
      return folly::makeUnexpected(errno);
    }
    return folly::makeExpected<int>(ret);
  } else {
    NOT_IMPLEMENTED();
  }
#else
  return fsync();
#endif
}

folly::Expected<std::string, int> OverlayFile::readFile() const {
  std::shared_ptr<Overlay> overlay = overlay_.lock();
  if (!overlay) {
    return folly::makeUnexpected(EIO);
  }
  if (std::holds_alternative<folly::File>(data_)) {
    auto& file = std::get<folly::File>(data_);
    IORequest req{overlay.get()};

    std::string out;
    if (!folly::readFile(file.fd(), out)) {
      return folly::makeUnexpected(errno);
    }
    return folly::makeExpected<int>(std::move(out));
  } else {
    NOT_IMPLEMENTED();
  }
}

} // namespace facebook::eden

#endif
