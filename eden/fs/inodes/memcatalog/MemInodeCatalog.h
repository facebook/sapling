/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/InodeCatalog.h"
#include "eden/fs/inodes/InodeNumber.h"

#include <folly/container/F14Map.h>

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}

class NonEmptyError : public std::exception {
 public:
  explicit NonEmptyError(std::string&& str)
      : message_(folly::to<std::string>(
            "Invalid operation on non-empty entity: ",
            str)) {}

  const char* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

/**
 * MemInodeCatalog provides interfaces to manipulate the overlay. It stores the
 * overlay's file system attributes and is responsible for obtaining and
 * releasing its locks ("initOverlay" and "close" respectively).
 */
class MemInodeCatalog : public InodeCatalog {
 public:
  explicit MemInodeCatalog() {}

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

  void removeChild(InodeNumber parent, PathComponentPiece childName) override;

  bool hasChild(InodeNumber parent, PathComponentPiece childName) override;

  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName) override;

  InodeNumber nextInodeNumber() override;

  /**
   * Scan filesystem changes when EdenFS is not running. This is only required
   * on Windows as ProjectedFS allows user to make changes under certain
   * directory when EdenFS is not running.
   */
  InodeNumber scanLocalChanges(
      std::shared_ptr<const EdenConfig> config,
      AbsolutePathPiece mountPath,
      bool windowsSymlinksEnabled,
      InodeCatalog::LookupCallback& callback) override;

  void maintenance() override {}

  std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) override;

 private:
  folly::Synchronized<folly::F14FastMap<InodeNumber, overlay::OverlayDir>>
      store_;
  std::atomic_uint64_t nextInode_{1};
};

} // namespace facebook::eden
