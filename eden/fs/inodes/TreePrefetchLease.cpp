/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/TreePrefetchLease.h"

#include "eden/fs/inodes/EdenMount.h"

namespace facebook::eden {

void TreePrefetchLease::release() noexcept {
  if (inode_) {
    inode_->getMount()->treePrefetchFinished();
  }
}

} // namespace facebook::eden
