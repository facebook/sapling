/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/PathFuncs.h"

#ifdef __APPLE__
#include <sys/mount.h>
#include <sys/param.h>
#elif !defined(_WIN32)
#include <sys/vfs.h>
#endif

namespace folly {
class File;
class IOBuf;
} // namespace folly

namespace facebook::eden {

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

  // Older overlay implementation only care about data storage but has little
  // understanding of the content it stores. A set of methods are added to allow
  // overlay implementation to optimize based on the semantic changes over the
  // data it stores.
  //
  // This method is used to indicate if the implementation supports these type
  // of operations (`*Child` methods).
  virtual bool supportsSemanticOperations() const = 0;

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

  /**
   * Load the directory content associated with the given `InodeNumber`
   */
  virtual std::optional<overlay::OverlayDir> loadOverlayDir(
      InodeNumber inodeNumber) = 0;

  /**
   * Remove the directory associated with the given `InodeNumber` and return
   * its content.
   */
  virtual std::optional<overlay::OverlayDir> loadAndRemoveOverlayDir(
      InodeNumber inodeNumber) = 0;

  /**
   * Save a directory content to overlay with the given `InodeNumber`
   */
  virtual void saveOverlayDir(
      InodeNumber inodeNumber,
      overlay::OverlayDir&& odir) = 0;

  /**
   * Remove the overlay directory record associated with the passed InodeNumber.
   */
  virtual void removeOverlayDir(InodeNumber inodeNumber) = 0;

  /**
   * Return if the overlay has a directory record of given InodeNumber.
   */
  virtual bool hasOverlayDir(InodeNumber inodeNumber) = 0;

  virtual void addChild(
      InodeNumber /* parent */,
      PathComponentPiece /* name */,
      overlay::OverlayEntry /* entry */) {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  virtual void removeChild(
      InodeNumber /* parent */,
      PathComponentPiece /* childName */) {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  virtual bool hasChild(
      InodeNumber /* parent */,
      PathComponentPiece /* childName */) {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  virtual void renameChild(
      InodeNumber /* src */,
      InodeNumber /* dst */,
      PathComponentPiece /* srcName */,
      PathComponentPiece /* destName */) {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  virtual void maintenance() {
    EDEN_BUG() << "UNIMPLEMENTED";
  }
};
} // namespace facebook::eden
