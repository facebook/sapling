/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "OverlayTestUtil.h"

namespace facebook::eden {

void debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode,
    AbsolutePathPiece path,
    std::ostringstream& out) {
  out << path << "\n";
  out << "  Inode number: " << rootInode << "\n";

  auto dir = overlay.loadOverlayDir(rootInode);
  out << "  Entries (" << dir.size() << " total):\n";

  auto dtypeToString = [](dtype_t dtype) noexcept -> const char* {
    switch (dtype) {
      case dtype_t::Dir:
        return "d";
      case dtype_t::Regular:
        return "f";
      default:
        return "?";
    }
  };

  for (const auto& [entryPath, entry] : dir) {
    auto permissions = entry.getInitialMode() & ~S_IFMT;
    out << "  " << std::dec << std::setw(11) << entry.getInodeNumber() << " "
        << dtypeToString(entry.getDtype()) << " " << std::oct << std::setw(4)
        << permissions << " " << entryPath << "\n";
  }
  for (const auto& [entryPath, entry] : dir) {
    if (entry.getDtype() == dtype_t::Dir) {
      debugDumpOverlayInodes(
          overlay, entry.getInodeNumber(), path + entryPath, out);
    }
  }
}

} // namespace facebook::eden
