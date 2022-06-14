/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <iomanip>
#include <sstream>

#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

void debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode,
    AbsolutePathPiece path,
    std::ostringstream& out);

inline std::string debugDumpOverlayInodes(
    Overlay& overlay,
    InodeNumber rootInode) {
  std::ostringstream out;
  debugDumpOverlayInodes(overlay, rootInode, AbsolutePathPiece{}, out);
  return out.str();
}

} // namespace facebook::eden
