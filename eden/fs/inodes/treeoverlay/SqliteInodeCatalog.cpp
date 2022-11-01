/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/SqliteInodeCatalog.h"

#include <folly/File.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/inodes/treeoverlay/WindowsFsck.h"
#include "eden/fs/utils/Bug.h"

namespace facebook::eden {

SqliteInodeCatalog::SqliteInodeCatalog(
    AbsolutePathPiece path,
    SqliteTreeStore::SynchronousMode mode)
    : store_{path, mode} {}

std::optional<InodeNumber> SqliteInodeCatalog::initOverlay(
    bool createIfNonExisting) {
  if (createIfNonExisting) {
    store_.createTableIfNonExisting();
  }
  initialized_ = true;
  return store_.loadCounters();
}

void SqliteInodeCatalog::close(std::optional<InodeNumber> /*nextInodeNumber*/) {
  store_.close();
}

std::optional<overlay::OverlayDir> SqliteInodeCatalog::loadOverlayDir(
    InodeNumber inodeNumber) {
  return store_.loadTree(inodeNumber);
}

std::optional<overlay::OverlayDir> SqliteInodeCatalog::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  return store_.loadAndRemoveTree(inodeNumber);
}

void SqliteInodeCatalog::saveOverlayDir(
    InodeNumber inodeNumber,
    overlay::OverlayDir&& odir) {
  return store_.saveTree(inodeNumber, std::move(odir));
}

void SqliteInodeCatalog::removeOverlayDir(InodeNumber inodeNumber) {
  store_.removeTree(inodeNumber);
}

bool SqliteInodeCatalog::hasOverlayDir(InodeNumber inodeNumber) {
  return store_.hasTree(inodeNumber);
}

void SqliteInodeCatalog::addChild(
    InodeNumber parent,
    PathComponentPiece name,
    overlay::OverlayEntry entry) {
  return store_.addChild(parent, name, entry);
}

void SqliteInodeCatalog::removeChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  return store_.removeChild(parent, childName);
}

bool SqliteInodeCatalog::hasChild(
    InodeNumber parent,
    PathComponentPiece childName) {
  return store_.hasChild(parent, childName);
}

void SqliteInodeCatalog::renameChild(
    InodeNumber src,
    InodeNumber dst,
    PathComponentPiece srcName,
    PathComponentPiece dstName) {
  return store_.renameChild(src, dst, srcName, dstName);
}

InodeNumber SqliteInodeCatalog::nextInodeNumber() {
  return store_.nextInodeNumber();
}

InodeNumber SqliteInodeCatalog::scanLocalChanges(
    std::shared_ptr<const EdenConfig> config,
    AbsolutePathPiece mountPath,
    FOLLY_MAYBE_UNUSED SqliteInodeCatalog::LookupCallback& callback) {
#ifdef _WIN32
  windowsFsckScanLocalChanges(config, *this, mountPath, callback);
#else
  (void)config;
  (void)mountPath;
#endif
  return store_.loadCounters();
}
} // namespace facebook::eden
