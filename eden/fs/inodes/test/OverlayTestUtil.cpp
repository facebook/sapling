/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/test/OverlayTestUtil.h"

namespace facebook::eden {

void debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode,
    folly::StringPiece path,
    std::ostringstream& out) {
  out << fmt::format("{}\n  Inode number: {}\n", path, rootInode);

  auto dir = overlay.loadOverlayDir(rootInode);
  out << fmt::format("  Entries ({} total):\n", dir.size());

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
    out << fmt::format(
        "{:>13} {}  {:3o} {}\n",
        entry.getInodeNumber(),
        dtypeToString(entry.getDtype()),
        permissions,
        entryPath.value());
  }
  for (const auto& [entryPath, entry] : dir) {
    if (entry.getDtype() == dtype_t::Dir) {
      debugDumpOverlayInodes(
          overlay,
          entry.getInodeNumber(),
          fmt::format("{}{}{}", path, path == "/" ? "" : "/", entryPath),
          out);
    }
  }
}

} // namespace facebook::eden
