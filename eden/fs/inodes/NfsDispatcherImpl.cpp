/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/NfsDispatcherImpl.h"

#include <folly/futures/Future.h>

#include "eden/common/utils/String.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/nfs/NfsUtils.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/NotImplemented.h"

namespace facebook::eden {

ImmediateFuture<struct stat> statHelper(
    const InodePtr& inode,
    const ObjectFetchContextPtr& context) {
  // TODO: stat is not safe to call on windows because it's gonna try to stat
  // the working copy. On NFS that's going to cause infinite recursion, and if I
  // had to bet probably blue screens. Needs to be fixed before we can call
  // stat.
#ifndef _WIN32
  return inode->stat(context);
#else
  (void)inode;
  (void)context;
  return makeImmediateFutureWith([]() -> struct stat { NOT_IMPLEMENTED(); });
#endif
}

NfsDispatcherImpl::NfsDispatcherImpl(EdenMount* mount)
    : NfsDispatcher(mount->getStats().copy(), mount->getClock()),
      mount_(mount),
      inodeMap_(mount_->getInodeMap()),
      allowAppleDouble_(mount_->getEdenConfig()->allowAppleDouble.getValue()) {}

ImmediateFuture<struct stat> NfsDispatcherImpl::getattr(
    InodeNumber ino,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupInode(ino).thenValue(
      [context = context.copy()](const InodePtr& inode) {
        return statHelper(inode, context);
      });
}

ImmediateFuture<NfsDispatcher::SetattrRes> NfsDispatcherImpl::setattr(
    InodeNumber ino,
    DesiredMetadata desired,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupInode(ino)
      .thenValue([desired = std::move(desired),
                  context = context.copy()](const InodePtr& inode) {
        // TODO(xavierd): Modify setattr to obtain pre stat of the file.
        return inode->setattr(desired, context);
      })
      .thenValue([](struct stat st) {
        return NfsDispatcher::SetattrRes{std::nullopt, st};
      });
}

ImmediateFuture<InodeNumber> NfsDispatcherImpl::getParent(
    InodeNumber ino,
    const ObjectFetchContextPtr& /*context*/) {
  return inodeMap_->lookupTreeInode(ino).thenValue(
      [](const TreeInodePtr& inode) {
        return inode->getParentRacy()->getNodeId();
      });
}

ImmediateFuture<std::tuple<InodeNumber, struct stat>> NfsDispatcherImpl::lookup(
    InodeNumber dir,
    PathComponent name,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir)
      .thenValue([name = std::move(name),
                  context = context.copy()](const TreeInodePtr& inode) {
        return inode->getOrLoadChild(name, context);
      })
      .thenValue([context = context.copy()](InodePtr&& inode) {
        auto statFut = statHelper(inode, context);
        return std::move(statFut).thenValue(
            [inode = std::move(inode)](
                struct stat stat) -> std::tuple<InodeNumber, struct stat> {
              inode->incFsRefcount();
              return {inode->getNodeId(), stat};
            });
      });
}

ImmediateFuture<std::string> NfsDispatcherImpl::readlink(
    InodeNumber ino,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [context = context.copy()](const FileInodePtr& inode) {
#ifndef _WIN32
        return inode->readlink(context);
#else
        // todo: enable readlink on Windows
        // this would read out of the working copy on windows. not what we
        // want on NFS.
        (void)inode;
        return makeImmediateFutureWith(
            []() -> std::string { NOT_IMPLEMENTED(); });
#endif
      });
}

ImmediateFuture<NfsDispatcher::ReadRes> NfsDispatcherImpl::read(
    InodeNumber ino,
    size_t size,
    FileOffset offset,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [context = context.copy(), size, offset](const FileInodePtr& inode) {
        return inode->read(size, offset, context)
            .thenValue(
                [](std::tuple<std::unique_ptr<folly::IOBuf>, bool>&& res) {
                  auto [data, isEof] = std::move(res);
                  return ReadRes{std::move(data), isEof};
                });
      });
}

ImmediateFuture<NfsDispatcher::WriteRes> NfsDispatcherImpl::write(
    InodeNumber ino,
    std::unique_ptr<folly::IOBuf> data,
    FileOffset offset,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupFileInode(ino).thenValue(
      [data = std::move(data), offset, context = context.copy()](
          const FileInodePtr& inode) mutable {
        // TODO(xavierd): Modify write to obtain pre and post stat of the
        // file.
        return inode->write(std::move(data), offset, context)
            .thenValue([](size_t written) {
              return WriteRes{written, std::nullopt, std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::CreateRes> NfsDispatcherImpl::create(
    InodeNumber dir,
    PathComponent name,
    mode_t mode,
    const ObjectFetchContextPtr& context) {
  // macOS loves sprinkling ._ (AppleDouble) files all over the repository,
  // prevent it from doing so.
  if (folly::kIsApple && !allowAppleDouble_ &&
      string_view{name.view()}.starts_with("._")) {
    return makeImmediateFuture<NfsDispatcher::CreateRes>(
        std::system_error(EACCES, std::generic_category()));
  }
  // Make sure that we're attempting to create a file.
  mode = S_IFREG | (0777 & mode);
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(), name = std::move(name), mode](
          const TreeInodePtr& inode) {
        // TODO(xavierd): Modify mknod to obtain the pre and post stat of the
        // directory.
        // Set dev to 0 as this is unused for a regular file.
        auto newFile = inode->mknod(name, mode, 0, InvalidationRequired::No);
        auto statFut = statHelper(newFile, context);
        return std::move(statFut).thenValue(
            [newFile = std::move(newFile)](struct stat&& stat) {
              newFile->incFsRefcount();
              return CreateRes{
                  newFile->getNodeId(),
                  std::move(stat),
                  std::nullopt,
                  std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::MkdirRes> NfsDispatcherImpl::mkdir(
    InodeNumber dir,
    PathComponent name,
    mode_t mode,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(), name = std::move(name), mode](
          const TreeInodePtr& inode) {
        // TODO(xavierd): Modify mkdir to obtain the pre and post stat of the
        // directory.
        auto newDir = inode->mkdir(name, mode, InvalidationRequired::No);
        auto statFut = statHelper(newDir, context);
        return std::move(statFut).thenValue([newDir = std::move(newDir)](
                                                struct stat&& stat) {
          newDir->incFsRefcount();
          return MkdirRes{
              newDir->getNodeId(), std::move(stat), std::nullopt, std::nullopt};
        });
      });
}

ImmediateFuture<NfsDispatcher::SymlinkRes> NfsDispatcherImpl::symlink(
    InodeNumber dir,
    PathComponent name,
    std::string data,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(),
       name = std::move(name),
       data = std::move(data)](const TreeInodePtr& inode) {
        // TODO(xavierd): Modify symlink to obtain the pre and post stat of the
        // directory.
        auto symlink = inode->symlink(name, data, InvalidationRequired::No);
        auto statFut = statHelper(symlink, context);
        return std::move(statFut).thenValue(
            [symlink = std::move(symlink)](struct stat&& stat) {
              symlink->incFsRefcount();
              return SymlinkRes{
                  symlink->getNodeId(),
                  std::move(stat),
                  std::nullopt,
                  std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::MknodRes> NfsDispatcherImpl::mknod(
    InodeNumber dir,
    PathComponent name,
    mode_t mode,
    dev_t rdev,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(), name = std::move(name), mode, rdev](
          const TreeInodePtr& inode) {
        // TODO(xavierd): Modify mknod to obtain the pre and post stat of the
        // directory.
        auto newFile = inode->mknod(name, mode, rdev, InvalidationRequired::No);
        auto statFut = statHelper(newFile, context);
        return std::move(statFut).thenValue(
            [newFile = std::move(newFile)](struct stat&& stat) {
              newFile->incFsRefcount();
              return MknodRes{
                  newFile->getNodeId(),
                  std::move(stat),
                  std::nullopt,
                  std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::UnlinkRes> NfsDispatcherImpl::unlink(
    InodeNumber dir,
    PathComponent name,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(),
       name = std::move(name)](const TreeInodePtr& inode) {
        return inode->unlink(name, InvalidationRequired::No, context)
            .thenValue([](auto&&) {
              // TODO(xavierd): Modify unlink to obtain the pre and post stat
              // of the directory.
              return NfsDispatcher::UnlinkRes{std::nullopt, std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::RmdirRes> NfsDispatcherImpl::rmdir(
    InodeNumber dir,
    PathComponent name,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(),
       name = std::move(name)](const TreeInodePtr& inode) {
        return inode->rmdir(name, InvalidationRequired::No, context)
            .thenValue([](auto&&) {
              // TODO(xavierd): Modify rmdir to obtain the pre and post stat of
              // the directory.
              return NfsDispatcher::RmdirRes{std::nullopt, std::nullopt};
            });
      });
}

ImmediateFuture<NfsDispatcher::RenameRes> NfsDispatcherImpl::rename(
    InodeNumber fromIno,
    PathComponent fromName,
    InodeNumber toIno,
    PathComponent toName,
    const ObjectFetchContextPtr& context) {
  auto fromDir = inodeMap_->lookupTreeInode(fromIno);
  return inodeMap_->lookupTreeInode(toIno)
      .thenValue([fromDir = std::move(fromDir),
                  fromName = std::move(fromName),
                  toName = std::move(toName),
                  context = context.copy()](TreeInodePtr&& toDirInode) mutable {
        return std::move(fromDir).thenValue(
            [fromName = std::move(fromName),
             toName = std::move(toName),
             toDirInode = std::move(toDirInode),
             context = context.copy()](const TreeInodePtr& fromDirInode) {
              return fromDirInode->rename(
                  fromName,
                  toDirInode,
                  toName,
                  InvalidationRequired::No,
                  context);
            });
      })
      .thenValue([](auto&&) {
        // TODO(xavierd): collect pre and post dir stats.
        return NfsDispatcher::RenameRes{};
      });
}

ImmediateFuture<NfsDispatcher::ReaddirRes> NfsDispatcherImpl::readdir(
    InodeNumber dir,
    FileOffset offset,
    uint32_t count,
    const ObjectFetchContextPtr& context) {
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(), offset, count](const TreeInodePtr& inode) {
        auto [dirList, isEof] = inode->nfsReaddir(
            NfsDirList{count, nfsv3Procs::readdir}, offset, context);
        return ReaddirRes{std::move(dirList), isEof};
      });
}

ImmediateFuture<NfsDispatcher::ReaddirRes> NfsDispatcherImpl::readdirplus(
    InodeNumber dir,
    FileOffset offset,
    uint32_t count,
    const ObjectFetchContextPtr& context) {
#ifndef _WIN32
  return inodeMap_->lookupTreeInode(dir).thenValue(
      [context = context.copy(), offset, count, this](
          const TreeInodePtr& inode) {
        auto [dirList, isEof] = inode->nfsReaddir(
            NfsDirList{count, nfsv3Procs::readdirplus}, offset, context);
        auto& dirListRef = dirList.getListRef();
        std::vector<ImmediateFuture<folly::Unit>> futuresVec{};
        for (auto& entry : dirListRef) {
          if (entry.name == "." || entry.name == "..") {
            futuresVec.push_back(
                this->getattr(InodeNumber{entry.fileid}, context)
                    .thenTry([&entry](folly::Try<struct stat> st) {
                      entry.name_attributes = statToPostOpAttr(st);
                      return folly::unit;
                    }));
          } else {
            futuresVec.push_back(
                inode->getOrLoadChild(PathComponent{entry.name}, context)
                    .thenValue(
                        [entry, context = context.copy()](InodePtr&& inodep) {
                          return statHelper(inodep, context);
                        })
                    .thenTry([&entry](folly::Try<struct stat> st) {
                      entry.name_attributes = statToPostOpAttr(st);
                      return folly::unit;
                    }));
          }
        }
        auto res = collectAllSafe(std::move(futuresVec));
        return std::move(res).thenValue(
            [dirList_2 = std::move(dirList),
             isEof_2 = isEof](std::vector<folly::Unit>&&) mutable {
              return ReaddirRes{std::move(dirList_2), isEof_2};
            });
      });
#else
  // TODO: implement readdirplus on Windows.
  // shouldn't be too hard, but left out for now since we don't use readdir plus
  // in production.
  (void)dir;
  (void)offset;
  (void)count;
  (void)context;
  return makeImmediateFutureWith(
      []() -> NfsDispatcher::ReaddirRes { NOT_IMPLEMENTED(); });
#endif
}

#ifdef _WIN32
// TODO: find a statfs definition for Windows?
struct statfs {};
#endif

ImmediateFuture<struct statfs> NfsDispatcherImpl::statfs(
    InodeNumber /*dir*/,
    const ObjectFetchContextPtr& /*context*/) {
#ifndef _WIN32
  // See the comment in FuseDispatcherImpl::statfs for why we gather the statFs
  // from the overlay.
  return mount_->getOverlay()->statFs();
#else
  // TODO: implement statfs on windows
  return makeImmediateFutureWith([]() -> struct statfs { NOT_IMPLEMENTED(); });
#endif
}

} // namespace facebook::eden
