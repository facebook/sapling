/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/OverlayCheckerUtil.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/ImmediateFuture.h"
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

class EdenConfig;

/**
 * Interface for tracking inode relationships.
 */
class InodeCatalog {
 public:
  InodeCatalog() = default;

  virtual ~InodeCatalog() = default;

  InodeCatalog(const InodeCatalog&) = delete;
  InodeCatalog& operator=(const InodeCatalog&) = delete;
  InodeCatalog(InodeCatalog&&) = delete;
  InodeCatalog&& operator=(InodeCatalog&&) = delete;

  // Older overlay implementation only care about data storage but has little
  // understanding of the content it stores. A set of methods are added to allow
  // overlay implementation to optimize based on the semantic changes over the
  // data it stores.
  //
  // This method is used to indicate if the implementation supports these type
  // of operations (`*Child` methods).
  virtual bool supportsSemanticOperations() const = 0;

  /**
   * Get all of the `InodeNumber`s corresponding to directories. This is only
   * implemented for SqliteInodeCatalog for use in OverlayChecker to facilitate
   * loading of all of the known inodes.
   */
  virtual std::vector<InodeNumber> getAllParentInodeNumbers() = 0;

  /**
   * Initialize the overlay, run necessary operations to bootstrap the overlay.
   * The `close` method should be used to clean up any acquired resource for the
   * overlay and persist `nextInodeNumber` if needed.
   *
   * If `bypassLockFile` is set, in the case of an already opened overlay,
   * errors will be reported but fixes will not be attempted. This is used by
   * the `eden_fsck` executable.
   *
   * Returns the next inode number to start at when allocating new inodes.  For
   * certain overlay implementations, the inode number may not be available
   * if EdenFS was not shutdown cleanly. In that case, `std::nullopt` will be
   * returned.
   */
  virtual std::optional<InodeNumber> initOverlay(
      bool createIfNonExisting,
      bool bypassLockFile = false) = 0;

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

  /**
   * Loads an inode for use during fsck via the OverlayChecker.
   */
  virtual std::optional<fsck::InodeInfo> loadInodeInfo(InodeNumber number) = 0;

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

  virtual InodeNumber nextInodeNumber() {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  using LookupCallbackValue =
      std::variant<std::shared_ptr<const Tree>, TreeEntry>;
  using LookupCallback = std::function<ImmediateFuture<LookupCallbackValue>(
      const std::shared_ptr<const Tree>&,
      RelativePathPiece)>;

  /**
   * Scan filesystem changes when EdenFS is not running. This is only required
   * on Windows as ProjectedFS allows user to make changes under certain
   * directory when EdenFS is not running.
   */
  virtual InodeNumber scanLocalChanges(
      FOLLY_MAYBE_UNUSED std::shared_ptr<const EdenConfig> config,
      FOLLY_MAYBE_UNUSED AbsolutePathPiece mountPath,
      FOLLY_MAYBE_UNUSED bool windowsSymlinksEnabled,
      FOLLY_MAYBE_UNUSED LookupCallback& callback) {
    EDEN_BUG() << "UNIMPLEMENTED";
  }

  virtual void maintenance() {
    EDEN_BUG() << "UNIMPLEMENTED";
  }
};
} // namespace facebook::eden
