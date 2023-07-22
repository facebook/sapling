/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/memcatalog/MemInodeCatalog.h"

namespace facebook::eden {

bool MemInodeCatalog::initialized() const {
  return true;
}

std::optional<InodeNumber> MemInodeCatalog::initOverlay(
    FOLLY_MAYBE_UNUSED bool createIfNonExisting,
    FOLLY_MAYBE_UNUSED bool bypassLockFile) {
  return std::nullopt;
}

void MemInodeCatalog::close(
    FOLLY_MAYBE_UNUSED std::optional<InodeNumber> inodeNumber) {}

std::optional<overlay::OverlayDir> MemInodeCatalog::loadOverlayDir(
    FOLLY_MAYBE_UNUSED InodeNumber inodeNumber) {
  return std::nullopt;
}

std::optional<overlay::OverlayDir> MemInodeCatalog::loadAndRemoveOverlayDir(
    InodeNumber inodeNumber) {
  auto result = loadOverlayDir(inodeNumber);
  removeOverlayDir(inodeNumber);
  return result;
}

void MemInodeCatalog::saveOverlayDir(
    FOLLY_MAYBE_UNUSED InodeNumber inodeNumber,
    FOLLY_MAYBE_UNUSED overlay::OverlayDir&& odir) {}

std::vector<InodeNumber> MemInodeCatalog::getAllParentInodeNumbers() {
  return {};
}

void MemInodeCatalog::removeOverlayDir(
    FOLLY_MAYBE_UNUSED InodeNumber inodeNumber) {}

bool MemInodeCatalog::hasOverlayDir(
    FOLLY_MAYBE_UNUSED InodeNumber inodeNumber) {
  return false;
}

std::optional<fsck::InodeInfo> MemInodeCatalog::loadInodeInfo(
    FOLLY_MAYBE_UNUSED InodeNumber number) {
  return std::nullopt;
}
} // namespace facebook::eden
