/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Optional.h>
#include <folly/Range.h>
#include "TreeInode.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook {
namespace eden {

namespace overlay {
class OverlayDir;
}

/** Manages the write overlay storage area.
 *
 * The overlay is where we store files that are not yet part of a snapshot.
 *
 * The contents of this storage layer are overlaid on top of the object store
 * snapshot that is active in a given mount point.
 *
 * There is one overlay area associated with each eden client instance.
 *
 * We use the Overlay to manage mutating the structure of the checkout;
 * each time we create or delete a directory entry, we do so through
 * the overlay class.
 *
 * The Overlay class keeps track of the mutated tree; if we mutate some
 * file "foo/bar/baz" then the Overlay records metadata about the list
 * of files in the root, the list of files in "foo", the list of files in
 * "foo/bar" and finally materializes "foo/bar/baz".
 */
class Overlay {
 public:
  explicit Overlay(AbsolutePathPiece localDir);

  /** Returns the path to the root of the Overlay storage area */
  const AbsolutePath& getLocalDir() const;

  void saveOverlayDir(fuse_ino_t inodeNumber, const TreeInode::Dir* dir);
  folly::Optional<TreeInode::Dir> loadOverlayDir(fuse_ino_t inodeNumber) const;

  void removeOverlayData(fuse_ino_t inodeNumber) const;

  /**
   * Get the path to the overlay file for the given inode
   */
  AbsolutePath getFilePath(fuse_ino_t inodeNumber) const;

  /**
   * Get the maximum inode number stored in the overlay.
   *
   * This is called when opening a mount point, to make sure that new inodes
   * handed out from this point forwards are always greater than any inodes
   * already tracked in the overlay.
   */
  fuse_ino_t getMaxRecordedInode();

 private:
  void initOverlay();
  bool isOldFormatOverlay() const;
  void readExistingOverlay(int infoFD);
  void initNewOverlay();
  folly::Optional<overlay::OverlayDir> deserializeOverlayDir(
      fuse_ino_t inodeNumber) const;

  /** path to ".eden/CLIENT/local" */
  AbsolutePath localDir_;
};
}
}
