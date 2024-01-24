/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Expected.h>
#include <folly/File.h>
#include <folly/portability/SysUio.h>
#include <variant>

#include "eden/common/utils/FileOffset.h"
#include "eden/fs/inodes/InodeNumber.h"

namespace folly {
class File;
}

namespace facebook::eden {

class Overlay;

/**
 * Class to manage IO reference counting for an Overlay to support closing the
 * Overlay class even if it is still in use.
 *
 * If an OverlayFile was created from a folly::File, this class will manage
 * reference counting for the underlying on-disk Overlay storage.
 */
class OverlayFile {
 public:
  OverlayFile() = default;
  explicit OverlayFile(folly::File file, std::weak_ptr<Overlay> overlay);
  explicit OverlayFile(InodeNumber ino, std::weak_ptr<Overlay> overlay);
  explicit OverlayFile(
      std::variant<folly::File, InodeNumber> data,
      std::weak_ptr<Overlay> overlay);

  OverlayFile(OverlayFile&&) = default;
  OverlayFile& operator=(OverlayFile&&) = default;

  folly::Expected<struct stat, int> fstat() const;
  folly::Expected<ssize_t, int>
  preadNoInt(void* buf, size_t n, FileOffset offset) const;
  folly::Expected<FileOffset, int> lseek(FileOffset offset, int whence) const;
  folly::Expected<ssize_t, int>
  pwritev(const iovec* iov, int iovcnt, FileOffset offset) const;
  folly::Expected<int, int> ftruncate(FileOffset length) const;
  folly::Expected<int, int> fsync() const;
  folly::Expected<int, int> fallocate(FileOffset offset, FileOffset length)
      const;
  folly::Expected<int, int> fdatasync() const;
  folly::Expected<std::string, int> readFile() const;

 private:
  OverlayFile(const OverlayFile&) = delete;
  OverlayFile& operator=(const OverlayFile&) = delete;

  /**
   * This will contain a folly::File if created from an Overlay with type
   * InodeCatalogType::Legacy or an InodeNumber if created from an Overlay with
   * type InodeCatalogType::LMDB
   */
  std::variant<folly::File, InodeNumber> data_;
  std::weak_ptr<Overlay> overlay_;
};
} // namespace facebook::eden
