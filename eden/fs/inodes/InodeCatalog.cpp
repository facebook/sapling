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
    OverlayEntrySource source) {
  overlay::OverlayDir odir;
  source([&](const std::string& name, const overlay::OverlayEntry& entry) {
    odir.entries()->emplace(name, entry);
  });
  saveOverlayDir(inodeNumber, std::move(odir));
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

} // namespace facebook::eden
