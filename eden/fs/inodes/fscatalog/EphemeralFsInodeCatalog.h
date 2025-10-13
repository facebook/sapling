/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <gtest/gtest_prod.h>
#include <optional>
#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#ifdef __APPLE__
#include <sys/mount.h>
#include <sys/param.h>
#else
#include <sys/vfs.h>
#endif

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}
class InodePath;
class FsFileContentStore;

/**
 * EphemeralFsInodeCatalog provides interfaces to manipulate the overlay. It
 * stores the overlay's file system attributes and is responsible for obtaining
 * and releasing its locks ("initOverlay" and "close" respectively).
 * EphemeralFsInodeCatalog works like a combination of a MemInodeCatalog with
 * a FSFileContentStore (directories are tracked in-memory, files are tracked
 * on disk). This means that upon shutdown (either purposeful or not),
 * uncommitted information stored in the overlay will be lost.
 */
class EphemeralFsInodeCatalog : public InodeCatalog {
 public:
  explicit EphemeralFsInodeCatalog(FsFileContentStore* core) : core_(core) {}

  bool supportsSemanticOperations() const override {
    return true;
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

  void addChild(
      InodeNumber parent,
      PathComponentPiece name,
      overlay::OverlayEntry entry) override;

  bool removeChild(InodeNumber parent, PathComponentPiece childName) override;

  bool hasChild(InodeNumber parent, PathComponentPiece childName) override;

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName) override;

  void maintenance() override {}

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
  FsFileContentStore* core_;
  folly::Synchronized<folly::F14FastMap<InodeNumber, overlay::OverlayDir>>
      store_;
};

} // namespace facebook::eden
