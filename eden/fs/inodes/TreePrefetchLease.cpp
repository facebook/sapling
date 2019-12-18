/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/TreePrefetchLease.h"

#include "eden/fs/inodes/EdenMount.h"

namespace facebook {
namespace eden {

void TreePrefetchLease::release() noexcept {
  if (inode_) {
    inode_->getMount()->treePrefetchFinished();
  }
}

} // namespace eden
} // namespace facebook
