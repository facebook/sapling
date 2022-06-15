/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/inodes/FuseDispatcherImpl.h"
#include <folly/logging/xlog.h>
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/SystemError.h"

using namespace folly;
using std::string;
using std::vector;

namespace facebook::eden {

namespace {

/** Compute a fuse_entry_out */
fuse_entry_out computeEntryParam(const FuseDispatcher::Attr& attr) {
  XDCHECK(attr.st.st_ino) << "We should never return a 0 inode to FUSE";
  fuse_entry_out entry = {};
  entry.nodeid = attr.st.st_ino;
  entry.generation = 0;
  auto fuse_attr = attr.asFuseAttr();
  entry.attr = fuse_attr.attr;
  entry.attr_valid = fuse_attr.attr_valid;
  entry.attr_valid_nsec = fuse_attr.attr_valid_nsec;
  entry.entry_valid = fuse_attr.attr_valid;
  entry.entry_valid_nsec = fuse_attr.attr_valid_nsec;
  return entry;
}

constexpr int64_t kBrokenInodeCacheSeconds = 5;

FuseDispatcher::Attr attrForInodeWithCorruptOverlay(InodeNumber ino) noexcept {
  struct stat st = {};
  st.st_ino = ino.get();
  st.st_mode = S_IFREG;
  return FuseDispatcher::Attr{st, kBrokenInodeCacheSeconds};
}
} // namespace

FuseDispatcherImpl::FuseDispatcherImpl(EdenMount* mount)
    : FuseDispatcher(mount->getStats()),
      mount_(mount),
      inodeMap_(mount_->getInodeMap()) {}

ImmediateFuture<FuseDispatcher::Attr> FuseDispatcherImpl::getattr(
    InodeNumber ino,
    ObjectFetchContext& context) {
  return inodeMap_->lookupInode(ino)
      .thenValue(
          [&context](const InodePtr& inode) { return inode->stat(context); })
      .thenValue(
          [](const struct stat& st) { return FuseDispatcher::Attr{st}; });
}

ImmediateFuture<uint64_t> FuseDispatcherImpl::opendir(
    InodeNumber /*ino*/,
    int /*flags*/) {
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

ImmediateFuture<folly::Unit> FuseDispatcherImpl::releasedir(
    InodeNumber /*ino*/,
    uint64_t /*fh*/) {
  return folly::unit;
}

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::lookup(
    uint64_t /*requestID*/,
    InodeNumber parent,
    PathComponentPiece namepiece,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent)
      .thenValue([name = PathComponent(namepiece),
                  &context](const TreeInodePtr& tree) {
        return tree->getOrLoadChild(name, context);
      })
      .thenValue([&context](const InodePtr& inode) {
        return makeImmediateFutureWith([&]() { return inode->stat(context); })
            .thenTry([inode](folly::Try<struct stat> maybeStat) {
              if (maybeStat.hasValue()) {
                inode->incFsRefcount();
                return computeEntryParam(
                    FuseDispatcher::Attr{maybeStat.value()});
              } else {
                // The most common case for stat() failing is if this file is
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
                           << "): " << maybeStat.exception().what();
                inode->incFsRefcount();
                return computeEntryParam(
                    attrForInodeWithCorruptOverlay(inode->getNodeId()));
              }
            });
      })
      .thenTry([](folly::Try<fuse_entry_out> try_) {
        if (auto* err = try_.tryGetExceptionObject<std::system_error>()) {
          if (isEnoent(*err)) {
            // Translate ENOENT into a successful response with an
            // inode number of 0 and a large entry_valid time, to let the kernel
            // cache this negative lookup result.
            fuse_entry_out entry = {};
            entry.attr_valid =
                std::numeric_limits<decltype(entry.attr_valid)>::max();
            entry.entry_valid =
                std::numeric_limits<decltype(entry.entry_valid)>::max();
            return folly::Try<fuse_entry_out>{entry};
          }
        }
        return try_;
      });
}

ImmediateFuture<FuseDispatcher::Attr> FuseDispatcherImpl::setattr(
    InodeNumber ino,
    const fuse_setattr_in& attr,
    ObjectFetchContext& context) {
  // Even though mounts are created with the nosuid flag, explicitly disallow
  // setting suid, sgid, and sticky bits on any inodes. This lets us avoid
  // explicitly clearing these bits on writes() which is required for correct
  // behavior under FUSE_HANDLE_KILLPRIV.
  if ((attr.valid & FATTR_MODE) &&
      (attr.mode & (S_ISUID | S_ISGID | S_ISVTX))) {
    folly::throwSystemErrorExplicit(EPERM, "Extra mode bits are disallowed");
  }

  return inodeMap_->lookupInode(ino)
      .thenValue([this, attr, &context](const InodePtr& inode) {
        auto fuseTimeToTimespec = [](uint64_t time, uint64_t ntime) {
          timespec spec;
          spec.tv_sec = time;
          spec.tv_nsec = ntime;
          return spec;
        };

        auto now = mount_->getClock().getRealtime();

        DesiredMetadata desired;
        if (attr.valid & FATTR_SIZE) {
          desired.size = attr.size;
        }
        if (attr.valid & FATTR_MODE) {
          desired.mode = attr.mode;
        }
        if (attr.valid & FATTR_UID) {
          desired.uid = attr.uid;
        }
        if (attr.valid & FATTR_GID) {
          desired.gid = attr.gid;
        }
        if (attr.valid & FATTR_ATIME) {
          desired.atime = fuseTimeToTimespec(attr.atime, attr.atimensec);
        } else if (attr.valid & FATTR_ATIME_NOW) {
          desired.atime = now;
        }
        if (attr.valid & FATTR_MTIME) {
          desired.mtime = fuseTimeToTimespec(attr.mtime, attr.mtimensec);
        } else if (attr.valid & FATTR_MTIME_NOW) {
          desired.mtime = now;
        }

        return inode->setattr(desired, context);
      })
      .thenValue([](struct stat&& stat) {
        return FuseDispatcher::Attr{std::move(stat)};
      });
}

void FuseDispatcherImpl::forget(InodeNumber ino, unsigned long nlookup) {
  inodeMap_->decFsRefcount(ino, nlookup);
}

ImmediateFuture<uint64_t> FuseDispatcherImpl::open(
    InodeNumber /*ino*/,
    int /*flags*/) {
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

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::create(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode,
    int /*flags*/,
    ObjectFetchContext& context) {
  // force 'mode' to be regular file, in which case rdev arg to mknod is ignored
  // (and thus can be zero)
  mode = S_IFREG | (07777 & mode);
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [mode, childName = PathComponent{name}, &context](
          const TreeInodePtr& inode) {
        auto child = inode->mknod(childName, mode, 0, InvalidationRequired::No);
        return child->stat(context).thenValue(
            [child](struct stat st) -> fuse_entry_out {
              child->incFsRefcount();
              return computeEntryParam(FuseDispatcher::Attr{st});
            });
      });
}

ImmediateFuture<BufVec> FuseDispatcherImpl::read(
    InodeNumber ino,
    size_t size,
    off_t off,
    ObjectFetchContext& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [&context, size, off](FileInodePtr&& inode) {
        return inode->read(size, off, context)
            .thenValue([](std::tuple<BufVec, bool>&& readRes) {
              return std::get<BufVec>(std::move(readRes));
            });
      });
}

ImmediateFuture<size_t> FuseDispatcherImpl::write(
    InodeNumber ino,
    folly::StringPiece data,
    off_t off,
    ObjectFetchContext& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [copy = data.str(), off, &context](FileInodePtr&& inode) {
        return inode->write(copy, off, context);
      });
}

ImmediateFuture<Unit> FuseDispatcherImpl::flush(
    InodeNumber /* ino */,
    uint64_t /* lock_owner */) {
  // Return ENOSYS from flush.
  // This will cause the kernel to stop sending future flush() calls.
  return ImmediateFuture<Unit>(makeSystemErrorExplicit(ENOSYS, "flush"));
}

ImmediateFuture<folly::Unit> FuseDispatcherImpl::fallocate(
    InodeNumber ino,
    uint64_t offset,
    uint64_t length,
    ObjectFetchContext& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [offset, length, &context](FileInodePtr inode) {
        return inode->fallocate(offset, length, context);
      });
}

ImmediateFuture<folly::Unit> FuseDispatcherImpl::fsync(
    InodeNumber ino,
    bool datasync) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [datasync](FileInodePtr inode) { return inode->fsync(datasync); });
}

ImmediateFuture<Unit> FuseDispatcherImpl::fsyncdir(
    InodeNumber /* ino */,
    bool /* datasync */) {
  // Return ENOSYS from fsyncdir. The kernel will stop sending them.
  //
  // In a possible future where the tree structure is stored in a SQLite
  // database, we could handle this request by waiting for SQLite's
  // write-ahead-log to be flushed.
  return ImmediateFuture<Unit>(makeSystemErrorExplicit(ENOSYS, "fsyncdir"));
}

ImmediateFuture<std::string> FuseDispatcherImpl::readlink(
    InodeNumber ino,
    bool kernelCachesReadlink,
    ObjectFetchContext& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [kernelCachesReadlink, &context](const FileInodePtr& inode) {
        // Only release the symlink blob after it's loaded if we can assume the
        // FUSE will cache the result in the kernel's page cache.
        return inode
            ->readlink(
                context,
                kernelCachesReadlink ? CacheHint::NotNeededAgain
                                     : CacheHint::LikelyNeededAgain)
            .semi();
      });
}

ImmediateFuture<FuseDirList> FuseDispatcherImpl::readdir(
    InodeNumber ino,
    FuseDirList&& dirList,
    off_t offset,
    uint64_t /*fh*/,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(ino).thenValue(
      [dirList = std::move(dirList), offset, &context](
          TreeInodePtr inode) mutable {
        return inode->fuseReaddir(std::move(dirList), offset, context);
      });
}

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::mknod(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode,
    dev_t rdev,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [childName = PathComponent{name}, mode, rdev, &context](
          const TreeInodePtr& inode) {
        auto child =
            inode->mknod(childName, mode, rdev, InvalidationRequired::No);
        return child->stat(context).thenValue(
            [child](struct stat st) -> fuse_entry_out {
              child->incFsRefcount();
              return computeEntryParam(FuseDispatcher::Attr{st});
            });
      });
}

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::mkdir(
    InodeNumber parent,
    PathComponentPiece name,
    mode_t mode,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [childName = PathComponent{name}, mode, &context](
          const TreeInodePtr& inode) {
        auto child = inode->mkdir(childName, mode, InvalidationRequired::No);
        return child->stat(context).thenValue([child](struct stat st) {
          child->incFsRefcount();
          return computeEntryParam(FuseDispatcher::Attr{st});
        });
      });
}

ImmediateFuture<folly::Unit> FuseDispatcherImpl::unlink(
    InodeNumber parent,
    PathComponentPiece name,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [&context, childName = PathComponent{name}](const TreeInodePtr& inode) {
        // No need to flush the kernel cache because FUSE will do that for us.
        return inode->unlink(childName, InvalidationRequired::No, context);
      });
}

ImmediateFuture<folly::Unit> FuseDispatcherImpl::rmdir(
    InodeNumber parent,
    PathComponentPiece name,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [&context, childName = PathComponent{name}](const TreeInodePtr& inode) {
        // No need to flush the kernel cache because FUSE will do that for us.
        return inode->rmdir(childName, InvalidationRequired::No, context);
      });
}

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::symlink(
    InodeNumber parent,
    PathComponentPiece name,
    StringPiece link,
    ObjectFetchContext& context) {
  return inodeMap_->lookupTreeInode(parent).thenValue(
      [linkContents = link.str(), childName = PathComponent{name}, &context](
          const TreeInodePtr& inode) {
        auto symlinkInode =
            inode->symlink(childName, linkContents, InvalidationRequired::No);
        symlinkInode->incFsRefcount();
        return symlinkInode->stat(context).thenValue(
            [symlinkInode](struct stat st) {
              return computeEntryParam(FuseDispatcher::Attr{st});
            });
      });
}

ImmediateFuture<folly::Unit> FuseDispatcherImpl::rename(
    InodeNumber parent,
    PathComponentPiece namePiece,
    InodeNumber newParent,
    PathComponentPiece newNamePiece,
    ObjectFetchContext& context) {
  // Start looking up both parents
  auto parentFuture = inodeMap_->lookupTreeInode(parent);
  auto newParentFuture = inodeMap_->lookupTreeInode(newParent);
  // Do the rename once we have looked up both parents.
  return std::move(parentFuture)
      .thenValue([npFuture = std::move(newParentFuture),
                  name = PathComponent{namePiece},
                  newName = PathComponent{newNamePiece},
                  &context](const TreeInodePtr& parent) mutable {
        return std::move(npFuture).thenValue(
            [parent, name, newName, &context](const TreeInodePtr& newParent) {
              return parent->rename(
                  name, newParent, newName, InvalidationRequired::No, context);
            });
      });
}

ImmediateFuture<fuse_entry_out> FuseDispatcherImpl::link(
    InodeNumber /*ino*/,
    InodeNumber /*newParent*/,
    PathComponentPiece newName) {
  validatePathComponentLength(newName);

  // We intentionally do not support hard links.
  // These generally cannot be tracked in source control (git or mercurial)
  // and are not portable to non-Unix platforms.
  folly::throwSystemErrorExplicit(
      EPERM, "hard links are not supported in eden mount points");
}

ImmediateFuture<string> FuseDispatcherImpl::getxattr(
    InodeNumber ino,
    StringPiece name,
    ObjectFetchContext& context) {
  return inodeMap_->lookupInode(ino).thenValue(
      [attrName = name.str(), &context](const InodePtr& inode) {
        return inode->getxattr(attrName, context);
      });
}

ImmediateFuture<vector<string>> FuseDispatcherImpl::listxattr(InodeNumber ino) {
  return inodeMap_->lookupInode(ino).thenValue(
      [](const InodePtr& inode) { return inode->listxattr(); });
}

ImmediateFuture<struct fuse_kstatfs> FuseDispatcherImpl::statfs(
    InodeNumber /*ino*/) {
  struct fuse_kstatfs info = {};

  // Pass through the overlay free space stats; this gives a more reasonable
  // estimation of available storage space than the zeroes that we'd report
  // otherwise.  This is important because eg: Finder on macOS inspects disk
  // space prior to initiating a copy and will refuse to start a copy if
  // the disk appears to be full.
  // The only exception is bfree as users are misinterpreting the data and
  // think that EdenFS is consumming 100+ GB of disk space. We thus expose that
  // EdenFS free space is the same as the overlay capacity.
  auto overlayStats = mount_->getOverlay()->statFs();
  info.blocks = overlayStats.f_blocks;
  info.bfree = overlayStats.f_blocks;
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
} // namespace facebook::eden

#endif
