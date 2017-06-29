/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenDispatcher.h"

#include <dirent.h>
#include <folly/Format.h>
#include <folly/experimental/logging/xlog.h>
#include <gflags/gflags.h>
#include <wangle/concurrent/CPUThreadPoolExecutor.h>
#include <wangle/concurrent/GlobalExecutor.h>
#include <shared_mutex>

#include "eden/fs/fuse/Channel.h"
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileHandle.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"

using namespace folly;
using facebook::eden::PathComponentPiece;
using facebook::eden::PathComponent;
using facebook::eden::RelativePath;
using std::string;
using std::vector;

DEFINE_int32(
    inode_reserve,
    1000000,
    "pre-size inode hash table for this many entries");

namespace facebook {
namespace eden {

EdenDispatcher::EdenDispatcher(EdenMount* mount)
    : Dispatcher(mount->getStats()),
      mount_(mount),
      inodeMap_(mount_->getInodeMap()) {}

namespace {

/** Compute a fuse_entry_param */
fuse_entry_param computeEntryParam(
    fuse_ino_t number,
    const fusell::Dispatcher::Attr& attr) {
  fuse_entry_param entry;
  entry.ino = number;
  entry.generation = 1;
  entry.attr = attr.st;
  entry.attr_timeout = attr.timeout;
  entry.entry_timeout = attr.timeout;
  return entry;
}
}

folly::Future<fusell::Dispatcher::Attr> EdenDispatcher::getattr(
    fuse_ino_t ino) {
  XLOG(DBG7) << "getattr(" << ino << ")";
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->getattr(); });
}

folly::Future<std::shared_ptr<fusell::DirHandle>> EdenDispatcher::opendir(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  XLOG(DBG7) << "opendir(" << ino << ")";
  return inodeMap_->lookupTreeInode(ino).then(
      [fi](const TreeInodePtr& inode) { return inode->opendir(fi); });
}

folly::Future<fuse_entry_param> EdenDispatcher::lookup(
    fuse_ino_t parent,
    PathComponentPiece namepiece) {
  XLOG(DBG7) << "lookup(" << parent << ", " << namepiece << ")";
  return inodeMap_->lookupTreeInode(parent)
      .then([name = PathComponent(namepiece)](const TreeInodePtr& tree) {
        return tree->getOrLoadChild(name);
      })
      .then([](const InodePtr& inode) {
        return inode->getattr().then([inode](fusell::Dispatcher::Attr attr) {
          inode->incFuseRefcount();
          return computeEntryParam(inode->getNodeId(), attr);
        });
      });
}

folly::Future<fusell::Dispatcher::Attr>
EdenDispatcher::setattr(fuse_ino_t ino, const struct stat& attr, int toSet) {
  XLOG(DBG7) << "setattr(" << ino << ")";
  return inodeMap_->lookupInode(ino).then([attr, toSet](const InodePtr& inode) {
    return inode->setattr(attr, toSet);
  });
}

folly::Future<folly::Unit> EdenDispatcher::forget(
    fuse_ino_t ino,
    unsigned long nlookup) {
  XLOG(DBG7) << "forget(" << ino << ", " << nlookup << ")";
  inodeMap_->decFuseRefcount(ino, nlookup);
  return Unit{};
}

folly::Future<std::shared_ptr<fusell::FileHandle>> EdenDispatcher::open(
    fuse_ino_t ino,
    const struct fuse_file_info& fi) {
  XLOG(DBG7) << "open(" << ino << ")";
  return inodeMap_->lookupFileInode(ino).then(
      [fi](const FileInodePtr& inode) { return inode->open(fi); });
}

folly::Future<fusell::Dispatcher::Create> EdenDispatcher::create(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    int flags) {
  XLOGF(DBG7, "create({}, {}, {:#x}, {:#x})", parent, name, mode, flags);
  return inodeMap_->lookupTreeInode(parent)
      .then([ childName = PathComponent{name}, mode, flags ](
          const TreeInodePtr& parentInode) {
        return parentInode->create(childName, mode, flags);
      })
      .then([=](TreeInode::CreateResult created) {
        fusell::Dispatcher::Create result;
        created.inode->incFuseRefcount();
        result.entry =
            computeEntryParam(created.inode->getNodeId(), created.attr);
        result.fh = std::move(created.file);
        return result;
      });
}

folly::Future<std::string> EdenDispatcher::readlink(fuse_ino_t ino) {
  XLOG(DBG7) << "readlink(" << ino << ")";
  return inodeMap_->lookupFileInode(ino).then(
      [](const FileInodePtr& inode) { return inode->readlink(); });
}

folly::Future<fuse_entry_param> EdenDispatcher::mknod(
    fuse_ino_t parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev) {
  XLOGF(DBG7, "mknod({}, {}, {:#x}, {:#x})", parent, name, mode, rdev);
  return inodeMap_->lookupTreeInode(parent).then(
      [ childName = PathComponent{name}, mode, rdev ](
          const TreeInodePtr& inode) {
        auto child = inode->mknod(childName, mode, rdev);
        return child->getattr().then([child](fusell::Dispatcher::Attr attr) {
          child->incFuseRefcount();
          return computeEntryParam(child->getNodeId(), attr);
        });
      });
}

folly::Future<fuse_entry_param>
EdenDispatcher::mkdir(fuse_ino_t parent, PathComponentPiece name, mode_t mode) {
  XLOGF(DBG7, "mkdir({}, {}, {:#x})", parent, name, mode);
  return inodeMap_->lookupTreeInode(parent).then(
      [ childName = PathComponent{name}, mode ](const TreeInodePtr& inode) {
        auto child = inode->mkdir(childName, mode);
        return child->getattr().then([child](fusell::Dispatcher::Attr attr) {
          child->incFuseRefcount();
          return computeEntryParam(child->getNodeId(), attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::unlink(
    fuse_ino_t parent,
    PathComponentPiece name) {
  XLOG(DBG7) << "unlink(" << parent << ", " << name << ")";
  return inodeMap_->lookupTreeInode(parent).then(
      [ this, childName = PathComponent{name} ](const TreeInodePtr& inode) {
        inode->unlink(childName);
      });
}

folly::Future<folly::Unit> EdenDispatcher::rmdir(
    fuse_ino_t parent,
    PathComponentPiece name) {
  XLOG(DBG7) << "rmdir(" << parent << ", " << name << ")";
  return inodeMap_->lookupTreeInode(parent)
      .then([childName = PathComponent{name}](const TreeInodePtr& inode) {
        return inode->rmdir(childName);
      });
}

folly::Future<fuse_entry_param> EdenDispatcher::symlink(
    fuse_ino_t parent,
    PathComponentPiece name,
    StringPiece link) {
  XLOG(DBG7) << "symlink(" << parent << ", " << name << ", " << link << ")";
  return inodeMap_->lookupTreeInode(parent).then(
      [ linkContents = link.str(),
        childName = PathComponent{name} ](const TreeInodePtr& inode) {
        auto symlinkInode = inode->symlink(childName, linkContents);
        symlinkInode->incFuseRefcount();
        return symlinkInode->getattr().then([symlinkInode](Attr&& attr) {
          return computeEntryParam(symlinkInode->getNodeId(), attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::rename(
    fuse_ino_t parent,
    PathComponentPiece namePiece,
    fuse_ino_t newParent,
    PathComponentPiece newNamePiece) {
  XLOG(DBG7) << "rename(" << parent << ", " << namePiece << ", " << newParent
             << ", " << newNamePiece << ")";
  // Start looking up both parents
  auto parentFuture = inodeMap_->lookupTreeInode(parent);
  auto newParentFuture = inodeMap_->lookupTreeInode(newParent);
  // Do the rename once we have looked up both parents.
  return parentFuture.then([
    npFuture = std::move(newParentFuture),
    name = PathComponent{namePiece},
    newName = PathComponent{newNamePiece}
  ](const TreeInodePtr& parent) mutable {
    return npFuture.then(
        [parent, name, newName](const TreeInodePtr& newParent) {
          parent->rename(name, newParent, newName);
        });
  });
}

folly::Future<fuse_entry_param> EdenDispatcher::link(
    fuse_ino_t ino,
    fuse_ino_t newParent,
    PathComponentPiece newName) {
  XLOG(DBG7) << "link(" << ino << ", " << newParent << ", " << newName << ")";
  // We intentionally do not support hard links.
  // These generally cannot be tracked in source control (git or mercurial)
  // and are not portable to non-Unix platforms.
  folly::throwSystemErrorExplicit(
      EPERM, "hard links are not supported in eden mount points");
}

Future<string> EdenDispatcher::getxattr(fuse_ino_t ino, StringPiece name) {
  XLOG(DBG7) << "getxattr(" << ino << ", " << name << ")";
  return inodeMap_->lookupInode(ino).then([attrName = name.str()](
      const InodePtr& inode) { return inode->getxattr(attrName); });
}

Future<vector<string>> EdenDispatcher::listxattr(fuse_ino_t ino) {
  XLOG(DBG7) << "listxattr(" << ino << ")";
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->listxattr(); });
}
}
}
