/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/fscatalog/FsInodeCatalog.h"

namespace facebook::eden {

class InodePath {
 public:
  explicit InodePath() noexcept;

  /**
   * The maximum path length for the path to a file inside the overlay
   * directory.
   *
   * This is 2 bytes for the initial subdirectory name, 1 byte for the '/',
   * 20 bytes for the inode number, and 1 byte for a null terminator.
   */
  static constexpr size_t kMaxPathLength =
      2 + 1 + FsFileContentStore::kMaxDecimalInodeNumberLength + 1;

  const char* c_str() const noexcept;
  /* implicit */ operator RelativePathPiece() const noexcept;

  std::array<char, kMaxPathLength>& rawData() noexcept;

 private:
  std::array<char, kMaxPathLength> path_;
};

/**
 * A fixed-size path for WAL (Write-Ahead Log) files in the overlay.
 * Paths are relative to the overlay local directory: "XX/<inode>.wal".
 * Same shard layout as InodePath, with a ".wal" suffix.
 */
class WalPath {
 public:
  explicit WalPath() noexcept;

  /**
   * The maximum path length: InodePath max + 4 bytes for ".wal" suffix.
   */
  static constexpr size_t kMaxPathLength = InodePath::kMaxPathLength + 4;

  const char* c_str() const noexcept;
  /* implicit */ operator RelativePathPiece() const noexcept;

  std::array<char, kMaxPathLength>& rawData() noexcept;

 private:
  std::array<char, kMaxPathLength> path_;
};

} // namespace facebook::eden
