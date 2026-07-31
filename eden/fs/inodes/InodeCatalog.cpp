/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeCatalog.h"

namespace facebook::eden {

void InodeCatalog::saveOverlayEntries(
    InodeNumber inodeNumber,
    size_t /*count*/,
    OverlayEntrySource source,
    bool crashSafe) {
  overlay::OverlayDir odir;
  source([&](const std::string& name, const overlay::OverlayEntry& entry) {
    odir.entries()->emplace(name, entry);
  });
  saveOverlayDir(inodeNumber, std::move(odir), crashSafe);
}

bool InodeCatalog::loadOverlayEntries(
    InodeNumber inodeNumber,
    OverlayEntryLoader loader) {
  auto odir = loadOverlayDir(inodeNumber);
  if (!odir) {
    return false;
  }
  loader(odir->entries()->size(), [&](OverlayEntryVisitor visitor) {
    for (auto& [name, entry] : *odir->entries()) {
      visitor(name, entry);
    }
  });
  return true;
}

std::optional<fsck::InodeInfo> InodeCatalog::loadInodeInfoAndEntries(
    InodeNumber inodeNumber,
    OverlayEntryLoader loader) {
  auto info = loadInodeInfo(inodeNumber);
  if (!info || info->type != fsck::InodeType::Dir) {
    return info;
  }
  if (!loadOverlayEntries(inodeNumber, loader)) {
    return fsck::InodeInfo{
        inodeNumber,
        fsck::InodeType::Error,
        "unable to load directory contents"};
  }
  return info;
}

} // namespace facebook::eden
