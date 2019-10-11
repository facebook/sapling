/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "EdenDispatcher.h"

#include <folly/Format.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <cstring>
#include <shared_mutex>

#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;
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
  fuse_entry_out entry = {};
  entry.nodeid = number.get();
  entry.generation = 0;
  auto fuse_attr = attr.asFuseAttr();
  entry.attr = fuse_attr.attr;
  entry.attr_valid = fuse_attr.attr_valid;
  entry.attr_valid_nsec = fuse_attr.attr_valid_nsec;
  entry.entry_valid = fuse_attr.attr_valid;
  entry.entry_valid_nsec = fuse_attr.attr_valid_nsec;
  return entry;
}

Dispatcher::Attr attrForInodeWithCorruptOverlay() noexcept {
  struct stat st;
  std::memset(&st, 0, sizeof(st));
  st.st_mode = S_IFREG;
  return Dispatcher::Attr{st};
}
} // namespace

folly::Future<Dispatcher::Attr> EdenDispatcher::getattr(InodeNumber ino) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "getattr({})", ino);
  return inodeMap_->lookupInode(ino).thenValue(
      [](const InodePtr& inode) { return inode->getattr(); });
}

folly::Future<uint64_t> EdenDispatcher::opendir(InodeNumber ino, int flags) {
  FB_LOGF(
      mount_->getStraceLogger(), DBG7, "opendir({}, flags={:x})", ino, flags);
#ifdef FUSE_NO_OPENDIR_SUPPORT
  if (getConnInfo().flags & FUSE_NO_OPENDIR_SUPPORT) {
    // If the kernel understands FUSE_NO_OPENDIR_SUPPORT, then returning ENOSYS
    // means that no further opendir() nor releasedir() calls will make it into
    // Eden.
    folly::throwSystemErrorExplicit(
        ENOSYS, "Eden opendir() calls are stateless and not required");
  }
#endif
  return 0;
}

folly::Future<folly::Unit> EdenDispatcher::releasedir(
    InodeNumber ino,
    uint64_t fh) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "releasedir({}, {})", ino, fh);
  return folly::unit;
}

folly::Future<fuse_entry_out> EdenDispatcher::lookup(
    InodeNumber parent,
    PathComponentPiece namepiece) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "lookup({}, {})", parent, namepiece);
  return inodeMap_->lookupTreeInode(parent)
      .thenValue([name = PathComponent(namepiece)](const TreeInodePtr& tree) {
        return tree->getOrLoadChild(name);
      })
      .thenValue([](const InodePtr& inode) {
        return folly::makeFutureWith([&]() { return inode->getattr(); })
            .thenTry([inode](folly::Try<Dispatcher::Attr> maybeAttr) {
              if (maybeAttr.hasValue()) {
                inode->incFuseRefcount();
                return computeEntryParam(inode->getNodeId(), maybeAttr.value());
              } else {
                // The most common case for getattr() failing is if this file is
                // materialized but the data for it in the overlay is missing
                // or corrupt.  This can happen after a hard reboot where the
                // overlay data was not synced to disk first.
                //
                // We intentionally want to return a result here rather than
                // failing; otherwise we can't return the inode number to the
                // kernel at all.  This blocks other operations on the file,
                // like FUSE_UNLINK.  By successfully returning from the
                // lookup we allow clients to remove this corrupt file with an
                // unlink operation.  (Even though FUSE_UNLINK does not require
                // the child inode number, the kernel does not appear to send a
                // FUSE_UNLINK request to us if it could not get the child inode
                // number first.)
                XLOG(WARN) << "error getting attributes for inode "
                           << inode->getNodeId() << " (" << inode->getLogPath()
                           << "): " << maybeAttr.exception().what();
                inode->incFuseRefcount();
                return computeEntryParam(
                    inode->getNodeId(), attrForInodeWithCorruptOverlay());
              }
            });
      })
      .thenError(
          folly::tag_t<std::system_error>{}, [](const std::system_error& err) {
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
  return inodeMap_->lookupInode(ino).thenValue(
      [attr](const InodePtr& inode) { return inode->setattr(attr); });
}

void EdenDispatcher::forget(InodeNumber ino, unsigned long nlookup) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "forget({}, {})", ino, nlookup);
  inodeMap_->decFuseRefcount(ino, nlookup);
}

folly::Future<uint64_t> EdenDispatcher::open(InodeNumber ino, int flags) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "open({}, flags={:x})", ino, flags);
#ifdef FUSE_NO_OPEN_SUPPORT
  if (getConnInfo().flags & FUSE_NO_OPEN_SUPPORT) {
    // If the kernel understands FUSE_NO_OPEN_SUPPORT, then returning ENOSYS
    // means that no further open() nor release() calls will make it into Eden.
    folly::throwSystemErrorExplicit(
        ENOSYS, "Eden open() calls are stateless and not required");
  }
#endif
  return 0;
}

folly::Future<fuse_entry_out> EdenDispatcher::create(
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
      .thenValue([childName = PathComponent{name}, mode, flags](
                     const TreeInodePtr& parentInode) {
        return parentInode->create(childName, mode, flags);
      })
      .thenValue([=](TreeInode::CreateResult created) {
        created.inode->incFuseRefcount();
        return computeEntryParam(created.inode->getNodeId(), created.attr);
      });
}

folly::Future<BufVec>
EdenDispatcher::read(InodeNumber ino, size_t size, off_t off) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "read({}, off={}, len={})",
      ino,
      off,
      size);
  return inodeMap_->lookupFileInode(ino).thenValue(
      [size, off](FileInodePtr&& inode) { return inode->read(size, off); });
}

folly::Future<size_t>
EdenDispatcher::write(InodeNumber ino, folly::StringPiece data, off_t off) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "write({}, off={}, len={})",
      ino,
      off,
      data.size());
  return inodeMap_->lookupFileInode(ino).thenValue(
      [copy = data.str(), off](FileInodePtr&& inode) {
        return inode->write(copy, off);
      });
}

Future<Unit> EdenDispatcher::flush(
    InodeNumber /* ino */,
    uint64_t /* lock_owner */) {
  // Return ENOSYS from flush.
  // This will cause the kernel to stop sending future flush() calls.
  return makeFuture<Unit>(makeSystemErrorExplicit(ENOSYS, "flush"));
}

folly::Future<folly::Unit> EdenDispatcher::fsync(
    InodeNumber ino,
    bool datasync) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "fsync({})", ino);
  return inodeMap_->lookupFileInode(ino).thenValue(
      [datasync](FileInodePtr inode) { return inode->fsync(datasync); });
}

Future<Unit> EdenDispatcher::fsyncdir(
    InodeNumber /* ino */,
    bool /* datasync */) {
  // Return ENOSYS from fsyncdir. The kernel will stop sending them.
  //
  // In a possible future where the tree structure is stored in a SQLite
  // database, we could handle this request by waiting for SQLite's
  // write-ahead-log to be flushed.
  return makeFuture<Unit>(makeSystemErrorExplicit(ENOSYS, "fsyncdir"));
}

folly::Future<std::string> EdenDispatcher::readlink(
    InodeNumber ino,
    bool kernelCachesReadlink) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "readlink({})", ino);
  return inodeMap_->lookupFileInode(ino).thenValue(
      [kernelCachesReadlink](const FileInodePtr& inode) {
        // Only release the symlink blob after it's loaded if we can assume the
        // FUSE will cache the result in the kernel's page cache.
        return inode->readlink(
            kernelCachesReadlink ? CacheHint::NotNeededAgain
                                 : CacheHint::LikelyNeededAgain);
      });
}

folly::Future<DirList> EdenDispatcher::readdir(
    InodeNumber ino,
    DirList&& dirList,
    off_t offset,
    uint64_t /*fh*/) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "readdir({}, {})", ino, offset);
  return inodeMap_->lookupTreeInode(ino).thenValue(
      [dirList = std::move(dirList), offset](TreeInodePtr inode) mutable {
        return inode->readdir(std::move(dirList), offset);
      });
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
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [childName = PathComponent{name}, mode, rdev](const TreeInodePtr& inode) {
        auto child = inode->mknod(childName, mode, rdev);
        return child->getattr().thenValue([child](Dispatcher::Attr attr) {
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
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [childName = PathComponent{name}, mode](const TreeInodePtr& inode) {
        auto child = inode->mkdir(childName, mode);
        return child->getattr().thenValue([child](Dispatcher::Attr attr) {
          child->incFuseRefcount();
          return computeEntryParam(child->getNodeId(), attr);
        });
      });
}

folly::Future<folly::Unit> EdenDispatcher::unlink(
    InodeNumber parent,
    PathComponentPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "unlink({}, {})", parent, name);
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [childName = PathComponent{name}](const TreeInodePtr& inode) {
        return inode->unlink(childName);
      });
}

folly::Future<folly::Unit> EdenDispatcher::rmdir(
    InodeNumber parent,
    PathComponentPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "rmdir({}, {})", parent, name);
  return inodeMap_->lookupTreeInode(parent).thenValue(
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
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [linkContents = link.str(),
       childName = PathComponent{name}](const TreeInodePtr& inode) {
        auto symlinkInode = inode->symlink(childName, linkContents);
        symlinkInode->incFuseRefcount();
        return symlinkInode->getattr().thenValue([symlinkInode](Attr&& attr) {
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
  return std::move(parentFuture)
      .thenValue([npFuture = std::move(newParentFuture),
                  name = PathComponent{namePiece},
                  newName = PathComponent{newNamePiece}](
                     const TreeInodePtr& parent) mutable {
        return std::move(npFuture).thenValue(
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

  validatePathComponentLength(newName);

  // We intentionally do not support hard links.
  // These generally cannot be tracked in source control (git or mercurial)
  // and are not portable to non-Unix platforms.
  folly::throwSystemErrorExplicit(
      EPERM, "hard links are not supported in eden mount points");
}

Future<string> EdenDispatcher::getxattr(InodeNumber ino, StringPiece name) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "getxattr({}, {})", ino, name);
  return inodeMap_->lookupInode(ino).thenValue(
      [attrName = name.str()](const InodePtr& inode) {
        return inode->getxattr(attrName);
      });
}

Future<vector<string>> EdenDispatcher::listxattr(InodeNumber ino) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "listxattr({})", ino);
  return inodeMap_->lookupInode(ino).thenValue(
      [](const InodePtr& inode) { return inode->listxattr(); });
}

folly::Future<struct fuse_kstatfs> EdenDispatcher::statfs(InodeNumber /*ino*/) {
  struct fuse_kstatfs info = {};

  // Pass through the overlay free space stats; this gives a more reasonable
  // estimation of available storage space than the zeroes that we'd report
  // otherwise.  This is important because eg: Finder on macOS inspects disk
  // space prior to initiating a copy and will refuse to start a copy if
  // the disk appears to be full.
  auto overlayStats = mount_->getOverlay()->statFs();
  info.blocks = overlayStats.f_blocks;
  info.bfree = overlayStats.f_bfree;
  info.bavail = overlayStats.f_bavail;
  info.files = overlayStats.f_files;
  info.ffree = overlayStats.f_ffree;

  // Suggest a large blocksize to software that looks at that kind of thing
  // bsize will be returned to applications that call pathconf() with
  // _PC_REC_MIN_XFER_SIZE
  info.bsize = getConnInfo().max_readahead;

  // The fragment size is returned as the _PC_REC_XFER_ALIGN and
  // _PC_ALLOC_SIZE_MIN pathconf() settings.
  // 4096 is commonly used by many filesystem types.
  info.frsize = 4096;

  // Ensure that namelen is set to a non-zero value.
  // The value we return here will be visible to programs that call pathconf()
  // with _PC_NAME_MAX.  Returning 0 will confuse programs that try to honor
  // this value.
  info.namelen = 255;

  return info;
}

} // namespace eden
} // namespace facebook
