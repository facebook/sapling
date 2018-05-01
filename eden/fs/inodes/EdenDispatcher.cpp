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

#include <folly/Format.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/GlobalExecutor.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <shared_mutex>

#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/inodes/EdenFileHandle.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;
using facebook::eden::PathComponent;
using facebook::eden::PathComponentPiece;
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

/** Compute a fuse_entry_out */
fuse_entry_out computeEntryParam(
    InodeNumber number,
    const Dispatcher::Attr& attr) {
  fuse_entry_out entry;
  entry.nodeid = number.get();
  entry.generation = 1;
  auto fuse_attr = attr.asFuseAttr();
  entry.attr = fuse_attr.attr;
  entry.attr_valid = fuse_attr.attr_valid;
  entry.attr_valid_nsec = fuse_attr.attr_valid_nsec;
  entry.entry_valid = fuse_attr.attr_valid;
  entry.entry_valid_nsec = fuse_attr.attr_valid_nsec;
  return entry;
}
} // namespace

folly::Future<Dispatcher::Attr> EdenDispatcher::getattr(InodeNumber ino) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "getattr({})", ino);
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->getattr(); });
}

folly::Future<std::shared_ptr<DirHandle>> EdenDispatcher::opendir(
    InodeNumber ino,
    int flags) {
  FB_LOGF(
      mount_->getStraceLogger(), DBG7, "opendir({}, flags={:x})", ino, flags);
  return inodeMap_->lookupTreeInode(ino).then(
      [](const TreeInodePtr& inode) { return inode->opendir(); });
}

folly::Future<fuse_entry_out> EdenDispatcher::lookup(
    InodeNumber parent,
    PathComponentPiece namepiece) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "lookup({}, {})", parent, namepiece);
  return inodeMap_->lookupTreeInode(parent)
      .then([name = PathComponent(namepiece)](const TreeInodePtr& tree) {
        return tree->getOrLoadChild(name);
      })
      .then([](const InodePtr& inode) {
        return inode->getattr().then([inode](Dispatcher::Attr attr) {
          inode->incFuseRefcount();
          // Preserve inode's life for the duration of the prefetch.
          inode->prefetch().ensure([inode] {});
          return computeEntryParam(inode->getNodeId(), attr);
        });
      })
      .onError([](const std::system_error& err) {
        // Translate ENOENT into a successful response with an
        // inode number of 0 and a large entry_valid time, to let the kernel
        // cache this negative lookup result.
        if (isEnoent(err)) {
          fuse_entry_out entry = {};
          entry.attr_valid =
              std::numeric_limits<decltype(entry.attr_valid)>::max();
          entry.entry_valid =
              std::numeric_limits<decltype(entry.entry_valid)>::max();
          return entry;
        }
        throw err;
      });
}

folly::Future<Dispatcher::Attr> EdenDispatcher::setattr(
    InodeNumber ino,
    const fuse_setattr_in& attr) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "setattr({})", ino);
  return inodeMap_->lookupInode(ino).then(
      [attr](const InodePtr& inode) { return inode->setattr(attr); });
}

void EdenDispatcher::forget(InodeNumber ino, unsigned long nlookup) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "forget({}, {})", ino, nlookup);
  inodeMap_->decFuseRefcount(ino, nlookup);
}

folly::Future<std::shared_ptr<FileHandle>> EdenDispatcher::open(
    InodeNumber ino,
    int flags) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "open({}, flags={:x})", ino, flags);
  return inodeMap_->lookupFileInode(ino).then(
      [flags](const FileInodePtr& inode) { return inode->open(flags); });
}

folly::Future<Dispatcher::Create> EdenDispatcher::create(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode,
    int flags) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "create({}, {}, {:#x}, {:#x})",
      parent,
      name,
      mode,
      flags);
  return inodeMap_->lookupTreeInode(parent)
      .then([childName = PathComponent{name}, mode, flags](
                const TreeInodePtr& parentInode) {
        return parentInode->create(childName, mode, flags);
      })
      .then([=](TreeInode::CreateResult created) {
        Dispatcher::Create result;
        created.inode->incFuseRefcount();
        result.entry =
            computeEntryParam(created.inode->getNodeId(), created.attr);
        result.fh = std::move(created.file);
        return result;
      });
}

folly::Future<std::string> EdenDispatcher::readlink(InodeNumber ino) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "readlink({})", ino);
  return inodeMap_->lookupFileInode(ino).then(
      [](const FileInodePtr& inode) { return inode->readlink(); });
}

folly::Future<fuse_entry_out> EdenDispatcher::mknod(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "mknod({}, {}, {:#x}, {:#x})",
      parent,
      name,
      mode,
      rdev);
  return inodeMap_->lookupTreeInode(parent).then(
      [childName = PathComponent{name}, mode, rdev](const TreeInodePtr& inode) {
        auto child = inode->mknod(childName, mode, rdev);
        return child->getattr().then([child](Dispatcher::Attr attr) {
          child->incFuseRefcount();
          return computeEntryParam(child->getNodeId(), attr);
        });
      });
}

folly::Future<fuse_entry_out> EdenDispatcher::mkdir(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "mkdir({}, {}, {:#x})",
      parent,
      name,
      mode);
  return inodeMap_->lookupTreeInode(parent).then(
      [childName = PathComponent{name}, mode](const TreeInodePtr& inode) {
        auto child = inode->mkdir(childName, mode);
        return child->getattr().then([child](Dispatcher::Attr attr) {
          child->incFuseRefcount();
          return computeEntryParam(child->getNodeId(), attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::unlink(
    InodeNumber parent,
    PathComponentPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "unlink({}, {})", parent, name);
  return inodeMap_->lookupTreeInode(parent).then(
      [this, childName = PathComponent{name}](const TreeInodePtr& inode) {
        return inode->unlink(childName);
      });
}

folly::Future<folly::Unit> EdenDispatcher::rmdir(
    InodeNumber parent,
    PathComponentPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "rmdir({}, {})", parent, name);
  return inodeMap_->lookupTreeInode(parent).then(
      [childName = PathComponent{name}](const TreeInodePtr& inode) {
        return inode->rmdir(childName);
      });
}

folly::Future<fuse_entry_out> EdenDispatcher::symlink(
    InodeNumber parent,
    PathComponentPiece name,
    StringPiece link) {
  FB_LOGF(
      mount_->getStraceLogger(), DBG7, "rmdir({}, {}, {})", parent, name, link);
  return inodeMap_->lookupTreeInode(parent).then(
      [linkContents = link.str(),
       childName = PathComponent{name}](const TreeInodePtr& inode) {
        auto symlinkInode = inode->symlink(childName, linkContents);
        symlinkInode->incFuseRefcount();
        return symlinkInode->getattr().then([symlinkInode](Attr&& attr) {
          return computeEntryParam(symlinkInode->getNodeId(), attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::rename(
    InodeNumber parent,
    PathComponentPiece namePiece,
    InodeNumber newParent,
    PathComponentPiece newNamePiece) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "rename({}, {}, {}, {})",
      parent,
      namePiece,
      newParent,
      newNamePiece);
  // Start looking up both parents
  auto parentFuture = inodeMap_->lookupTreeInode(parent);
  auto newParentFuture = inodeMap_->lookupTreeInode(newParent);
  // Do the rename once we have looked up both parents.
  return parentFuture.then([npFuture = std::move(newParentFuture),
                            name = PathComponent{namePiece},
                            newName = PathComponent{newNamePiece}](
                               const TreeInodePtr& parent) mutable {
    return npFuture.then(
        [parent, name, newName](const TreeInodePtr& newParent) {
          return parent->rename(name, newParent, newName);
        });
  });
}

folly::Future<fuse_entry_out> EdenDispatcher::link(
    InodeNumber ino,
    InodeNumber newParent,
    PathComponentPiece newName) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "link({}, {}, {})",
      ino,
      newParent,
      newName);

  // We intentionally do not support hard links.
  // These generally cannot be tracked in source control (git or mercurial)
  // and are not portable to non-Unix platforms.
  folly::throwSystemErrorExplicit(
      EPERM, "hard links are not supported in eden mount points");
}

Future<string> EdenDispatcher::getxattr(InodeNumber ino, StringPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "getxattr({}, {})", ino, name);
  return inodeMap_->lookupInode(ino).then(
      [attrName = name.str()](const InodePtr& inode) {
        return inode->getxattr(attrName);
      });
}

Future<vector<string>> EdenDispatcher::listxattr(InodeNumber ino) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "listxattr({})", ino);
  return inodeMap_->lookupInode(ino).then(
      [](const InodePtr& inode) { return inode->listxattr(); });
}
} // namespace eden
} // namespace facebook
