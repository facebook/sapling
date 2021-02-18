/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/NfsDispatcherImpl.h"

#include <folly/futures/Future.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"

namespace facebook::eden {

NfsDispatcherImpl::NfsDispatcherImpl(EdenMount* mount)
    : NfsDispatcher(mount->getStats()),
      mount_(mount),
      inodeMap_(mount_->getInodeMap()) {}

folly::Future<struct stat> NfsDispatcherImpl::getattr(
    InodeNumber ino,
    ObjectFetchContext& context) {
  return inodeMap_->lookupInode(ino).thenValue(
      [&context](const InodePtr& inode) { return inode->stat(context); });
}

} // namespace facebook::eden

#endif
