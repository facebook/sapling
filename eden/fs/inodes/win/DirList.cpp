/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "DirList.h"
#include "eden/fs/fuse/InodeNumber.h"

using folly::StringPiece;

namespace facebook {
namespace eden {

void DirList::add(StringPiece name, uint64_t inode, dtype_t type) {
  list_.emplace_back(name, inode, type);
}

} // namespace eden
} // namespace facebook
