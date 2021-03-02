/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#ifdef __APPLE__
#include <sys/mount.h>
#include <sys/param.h>
#elif !defined(_WIN32)
#include <sys/vfs.h>
#endif

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class File;
class IOBuf;
} // namespace folly

namespace facebook::eden {

namespace overlay {
class OverlayDir;
}

struct InodeNumber;

/**
 * Overlay interface for different overlay implementations.
 */
class IOverlay {
 public:
  IOverlay() = default;

  virtual ~IOverlay() = default;

  IOverlay(const IOverlay&) = delete;
  IOverlay& operator=(const IOverlay&) = delete;
  IOverlay(IOverlay&&) = delete;
  IOverlay&& operator=(IOverlay&&) = delete;

  /**
   * Initialize the overlay, run necessary operations to bootstrap the overlay.
   * The `close` method should be used to clean up any acquired resource for the
   * overlay and persist `nextInodeNumber` if needed.
   *
   * Returns the next inode number to start at when allocating new inodes.  For
   * certain overlay implementations, the inode number may not be available
   * if EdenFS was not shutdown cleanly. In that case, `std::nullopt` will be
   * returned.
   */
  virtual std::optional<InodeNumber> initOverlay(bool createIfNonExisting) = 0;

  /**
   * Gracefully, shutdown the overlay, persisting the overlay's
   * nextInodeNumber.
   */
  virtual void close(std::optional<InodeNumber> nextInodeNumber) = 0;

  /**
   * If Overlay initialized - i.e., is cleanup (close) necessary.
   */
  virtual bool initialized() const = 0;

  virtual const AbsolutePath& getLocalDir() const = 0;

  /**
   * Load the directory content associated with the given `InodeNumber`
   */
  virtual std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) = 0;

  /**
   * Save a directory content to overlay with the given `InodeNumber`
   */
  virtual void saveOverlayDir(
      InodeNumber inodeNumber,
      const overlay::OverlayDir& odir) = 0;

  /**
   * Remove the overlay record associated with the passed InodeNumber.
   */
  virtual void removeOverlayData(InodeNumber inodeNumber) = 0;

  /**
   * Return if the overlay has a record of given InodeNumber.
   */
  virtual bool hasOverlayData(InodeNumber inodeNumber) = 0;

#ifndef _WIN32
  /**
   * Helper function that creates an overlay file for a new FileInode.
   */
  virtual folly::File createOverlayFile(
      InodeNumber inodeNumber,
      folly::ByteRange contents) = 0;

  /**
   * Helper function to write an overlay file for a FileInode with existing
   * contents.
   */
  virtual folly::File createOverlayFile(
      InodeNumber inodeNumber,
      const folly::IOBuf& contents) = 0;

  /**
   * Helper function that opens an existing overlay file,
   * checks if the file has valid header, and returns the file.
   */
  virtual folly::File openFile(
      InodeNumber inodeNumber,
      folly::StringPiece headerId) = 0;

  /**
   * Open an existing overlay file without verifying the header.
   */
  virtual folly::File openFileNoVerify(InodeNumber inodeNumber) = 0;

  /**
   * call statfs(2) on the filesystem in which the overlay is located
   */
  virtual struct statfs statFs() const = 0;
#endif

  /**
   * TODO(zeyi): delete this.
   *
   * This is required in current SQLite implementation due to the lack of fsck.
   * We can remove this once we can reliable get inode number.
   */
  virtual void updateUsedInodeNumber(uint64_t /* usedInodeNumber */) {}
};
} // namespace facebook::eden
