/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/portability/SysUio.h>
namespace folly {
class File;
}

namespace facebook {
namespace eden {

class Overlay;

class OverlayFile {
 public:
  OverlayFile() = default;
  explicit OverlayFile(folly::File file);

  int fstat(struct stat* buf) const;
  ssize_t preadNoInt(void* buf, size_t n, off_t offset) const;
  off_t lseek(off_t offset, int whence) const;
  ssize_t pwritev(const iovec* iov, int iovcnt, off_t offset) const;
  int ftruncate(off_t length) const;
  int fsync() const;
  int fdatasync() const;
  bool readFile(std::string& out) const;

 private:
  folly::File file_;
};
} // namespace eden
} // namespace facebook
