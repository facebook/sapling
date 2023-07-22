/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/InodeNumber.h"

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}

/**
 * MemInodeCatalog provides interfaces to manipulate the overlay. It stores the
 * overlay's file system attributes and is responsible for obtaining and
 * releasing its locks ("initOverlay" and "close" respectively).
 0*/
class MemInodeCatalog : public InodeCatalog {
 public:
  explicit MemInodeCatalog() {}

  bool supportsSemanticOperations() const override {
    return false;
  }

  std::vector<InodeNumber> getAllParentInodeNumbers() override;

  /**
   * Returns the next inode number to start at when allocating new inodes.
   */
  std::optional<InodeNumber> initOverlay(
      bool createIfNonExisting,
      bool bypassLockFile = false) override;

  /**
   *  Gracefully, shutdown the overlay, persisting the overlay's
   * nextInodeNumber.
   */
  void close(std::optional<InodeNumber> nextInodeNumber) override;

  /**
   * Was MemInodeCatalog initialized - i.e., is cleanup (close) necessary.
   */
  bool initialized() const override;

  std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) override;

  std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) override;

  void saveOverlayDir(InodeNumber inodeNumber, overlay::OverlayDir&& odir)
      override;

  /**
   * Remove the overlay directory data associated with the passed InodeNumber.
   */
  void removeOverlayDir(InodeNumber inodeNumber) override;

  bool hasOverlayDir(InodeNumber inodeNumber) override;

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
};

} // namespace facebook::eden
