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
#include "eden/fs/inodes/FileInode.h"
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

folly::Future<InodeNumber> NfsDispatcherImpl::getParent(
    InodeNumber ino,
    ObjectFetchContext& /*context*/) {
  return inodeMap_->lookupTreeInode(ino).thenValue(
      [](const TreeInodePtr& inode) {
        return inode->getParentRacy()->getNodeId();
      });
}

folly::Future<std::tuple<InodeNumber, struct stat>> NfsDispatcherImpl::lookup(
    InodeNumber dir,
    PathComponent name,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(dir)
      .thenValue([name = std::move(name), &context](const TreeInodePtr& inode) {
        return inode->getOrLoadChild(name, context);
      })
      .thenValue([&context](const InodePtr& inode) {
        return inode->stat(context).thenValue(
            [ino = inode->getNodeId()](
                struct stat stat) -> std::tuple<InodeNumber, struct stat> {
              return {ino, stat};
            });
      });
}

folly::Future<std::string> NfsDispatcherImpl::readlink(
    InodeNumber ino,
    ObjectFetchContext& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [&context](const FileInodePtr& inode) {
        return inode->readlink(context);
      });
}

folly::Future<NfsDispatcher::MkdirRes> NfsDispatcherImpl::mkdir(
    InodeNumber dir,
    PathComponent name,
    mode_t mode,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [&context, name = std::move(name), mode](const TreeInodePtr& inode) {
        // TODO(xavierd): Modify mkdir to obtain the pre and post stat of the
        // directory.
        auto newDir = inode->mkdir(name, mode, InvalidationRequired::No);
        auto statFut = newDir->stat(context);
        return std::move(statFut).thenValue([newDir = std::move(newDir)](
                                                struct stat&& stat) {
          return MkdirRes{
              newDir->getNodeId(), std::move(stat), std::nullopt, std::nullopt};
        });
      });
}

} // namespace facebook::eden

#endif
