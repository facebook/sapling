/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include <cpptoml.h> // @manual=fbsource//third-party/cpptoml:cpptoml
#include <folly/logging/xlog.h>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook::eden {

namespace {

const RelativePath kDotEdenConfigPath{".eden/config"};
const std::string kConfigRootPath{"root"};
const std::string kConfigSocketPath{"socket"};
const std::string kConfigClientPath{"client"};
const std::string kConfigTable{"Config"};

std::string makeDotEdenConfig(EdenMount& mount) {
  auto repoPath = mount.getPath();
  auto socketPath = mount.getServerState()->getSocketPath();
  auto clientPath = mount.getConfig()->getClientDirectory();

  auto rootTable = cpptoml::make_table();
  auto configTable = cpptoml::make_table();
  configTable->insert(kConfigRootPath, repoPath.c_str());
  configTable->insert(kConfigSocketPath, socketPath.c_str());
  configTable->insert(kConfigClientPath, clientPath.c_str());
  rootTable->insert(kConfigTable, configTable);

  std::ostringstream stream;
  stream << *rootTable;
  return stream.str();
}

} // namespace

PrjfsDispatcherImpl::PrjfsDispatcherImpl(EdenMount* mount)
    : PrjfsDispatcher(mount->getStats()),
      mount_{mount},
      dotEdenConfig_{makeDotEdenConfig(*mount)} {}

folly::Future<std::vector<FileMetadata>> PrjfsDispatcherImpl::opendir(
    RelativePathPiece path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context).thenValue([](const InodePtr inode) {
    auto treePtr = inode.asTreePtr();
    return treePtr->readdir();
  });
}

folly::Future<std::optional<LookupResult>> PrjfsDispatcherImpl::lookup(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue(
          [&context](const InodePtr inode) mutable
          -> folly::Future<std::optional<LookupResult>> {
            return inode->stat(context).thenValue(
                [inode = std::move(inode)](struct stat&& stat) {
                  size_t size = stat.st_size;
                  // Ensure that the OS has a record of the canonical
                  // file name, and not just whatever case was used to
                  // lookup the file
                  auto inodeMetadata =
                      InodeMetadata{*inode->getPath(), size, inode->isDir()};
                  auto incFsRefcount = [inode = std::move(inode)] {
                    inode->incFsRefcount();
                  };
                  return LookupResult{
                      std::move(inodeMetadata), std::move(incFsRefcount)};
                });
          })
      .thenError(
          folly::tag_t<std::system_error>{},
          [path = std::move(path), this](const std::system_error& ex)
              -> folly::Future<std::optional<LookupResult>> {
            if (isEnoent(ex)) {
              if (path == kDotEdenConfigPath) {
                return folly::makeFuture(LookupResult{
                    InodeMetadata{
                        std::move(path), dotEdenConfig_.length(), false},
                    [] {}});
              } else {
                XLOG(DBG6) << path << ": File not found";
                return folly::makeFuture(std::nullopt);
              }
            }
            return folly::makeFuture<std::optional<LookupResult>>(ex);
          });
}

folly::Future<bool> PrjfsDispatcherImpl::access(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue([](const InodePtr) { return true; })
      .thenError(
          folly::tag_t<std::system_error>{},
          [path = std::move(path)](const std::system_error& ex) {
            if (isEnoent(ex)) {
              if (path == kDotEdenConfigPath) {
                return folly::makeFuture(true);
              }
              return folly::makeFuture(false);
            }
            return folly::makeFuture<bool>(ex);
          });
}

folly::Future<std::string> PrjfsDispatcherImpl::read(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue([&context](const InodePtr inode) {
        auto fileInode = inode.asFilePtr();
        return fileInode->readAll(context);
      })
      .thenError(
          folly::tag_t<std::system_error>{},
          [path = std::move(path), this](const std::system_error& ex) {
            if (isEnoent(ex) && path == kDotEdenConfigPath) {
              return folly::makeFuture<std::string>(
                  std::string(dotEdenConfig_));
            }
            return folly::makeFuture<std::string>(ex);
          });
}

namespace {
folly::Future<TreeInodePtr> createDirInode(
    const EdenMount& mount,
    const RelativePathPiece path,
    ObjectFetchContext& context) {
  return mount.getInode(path, context)
      .thenValue([](const InodePtr inode) { return inode.asTreePtr(); })
      .thenError(
          folly::tag_t<std::system_error>{},
          [=, &mount](const std::system_error& ex) {
            if (!isEnoent(ex)) {
              return folly::makeFuture<TreeInodePtr>(ex);
            }

            mount.getStats()
                ->getChannelStatsForCurrentThread()
                .outOfOrderCreate.addValue(1);
            XLOG(DBG2) << "Out of order directory creation notification for: "
                       << path;

            /*
             * ProjectedFS notifications are asynchronous and sent after the
             * fact. This means that we can get a notification on a
             * file/directory before the parent directory notification has been
             * completed. This should be a very rare event and thus the code
             * below is pessimistic and will try to create all parent
             * directories.
             */

            auto fut = folly::makeFuture(mount.getRootInode());
            for (auto parent : path.paths()) {
              fut = std::move(fut).thenValue([parent](TreeInodePtr treeInode) {
                try {
                  auto inode = treeInode->mkdir(
                      parent.basename(), _S_IFDIR, InvalidationRequired::No);
                  inode->incFsRefcount();
                } catch (const std::system_error& ex) {
                  if (ex.code().value() != EEXIST) {
                    throw;
                  }
                }

                return treeInode->getOrLoadChildTree(parent.basename());
              });
            }

            return fut;
          });
}

folly::Future<folly::Unit> createFile(
    const EdenMount& mount,
    const RelativePathPiece path,
    bool isDirectory,
    ObjectFetchContext& context) {
  return createDirInode(mount, path.dirname(), context)
      .thenValue([=](const TreeInodePtr treeInode) {
        if (isDirectory) {
          try {
            auto inode = treeInode->mkdir(
                path.basename(), _S_IFDIR, InvalidationRequired::No);
            inode->incFsRefcount();
          } catch (const std::system_error& ex) {
            /*
             * If a concurrent createFile for a child of this directory finished
             * before this one, the directory will already exist. This is not an
             * error.
             */
            if (ex.code().value() != EEXIST) {
              return folly::makeFuture<folly::Unit>(ex);
            }
          }
        } else {
          auto inode = treeInode->mknod(
              path.basename(), _S_IFREG, 0, InvalidationRequired::No);
          inode->incFsRefcount();
        }

        return folly::makeFuture(folly::unit);
      });
}

folly::Future<folly::Unit> materializeFile(
    const EdenMount& mount,
    const RelativePathPiece path,
    ObjectFetchContext& context) {
  return mount.getInode(path, context).thenValue([](const InodePtr inode) {
    auto fileInode = inode.asFilePtr();
    fileInode->materialize();
    return folly::unit;
  });
}

folly::Future<folly::Unit> renameFile(
    const EdenMount& mount,
    const RelativePath oldPath,
    const RelativePath newPath,
    ObjectFetchContext& context) {
  auto oldParentInode = createDirInode(mount, oldPath.dirname(), context);
  auto newParentInode = createDirInode(mount, newPath.dirname(), context);

  return folly::collect(oldParentInode, newParentInode)
      .via(mount.getThreadPool().get())
      .thenValue([oldPath = std::move(oldPath), newPath = std::move(newPath)](
                     const std::tuple<TreeInodePtr, TreeInodePtr> inodes) {
        auto& oldParentTreePtr = std::get<0>(inodes);
        auto& newParentTreePtr = std::get<1>(inodes);
        // TODO(xavierd): In the case where the oldPath is actually being
        // created in another thread, EdenFS simply might not know about
        // it at this point. Creating the file and renaming it at this
        // point won't help as the other thread will re-create it. In the
        // future, we may want to try, wait a bit and retry, or re-think
        // this and somehow order requests so the file creation always
        // happens before the rename.
        //
        // This should be *extremely* rare, for now let's just let it
        // error out.
        return oldParentTreePtr->rename(
            oldPath.basename(),
            newParentTreePtr,
            newPath.basename(),
            InvalidationRequired::No);
      });
}

folly::Future<folly::Unit> removeFile(
    const EdenMount& mount,
    const RelativePathPiece path,
    bool isDirectory,
    ObjectFetchContext& context) {
  return mount.getInode(path.dirname(), context)
      .thenValue([path, isDirectory, &context](const InodePtr inode) {
        auto treeInodePtr = inode.asTreePtr();
        if (isDirectory) {
          return treeInodePtr->rmdir(
              path.basename(), InvalidationRequired::No, context);
        } else {
          return treeInodePtr->unlink(
              path.basename(), InvalidationRequired::No, context);
        }
      });
}

} // namespace

folly::Future<folly::Unit> PrjfsDispatcherImpl::newFileCreated(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool isDirectory,
    ObjectFetchContext& context) {
  return createFile(*mount_, relPath, isDirectory, context);
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::fileOverwritten(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    ObjectFetchContext& context) {
  return materializeFile(*mount_, relPath, context);
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::fileHandleClosedFileModified(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    ObjectFetchContext& context) {
  return materializeFile(*mount_, relPath, context);
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  // When files are moved in and out of the repo, the rename paths are
  // empty, handle these like creation/removal of files.
  if (oldPath.empty()) {
    return createFile(*mount_, newPath, isDirectory, context);
  } else if (newPath.empty()) {
    return removeFile(*mount_, oldPath, isDirectory, context);
  } else {
    return renameFile(*mount_, std::move(oldPath), std::move(newPath), context);
  }
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::preRename(
    RelativePath oldPath,
    RelativePath newPath,
    bool /*isDirectory*/,
    ObjectFetchContext& /*context*/) {
  return folly::unit;
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::fileHandleClosedFileDeleted(
    RelativePath oldPath,
    RelativePath /*destPath*/,
    bool isDirectory,
    ObjectFetchContext& context) {
  return removeFile(*mount_, oldPath, isDirectory, context);
}

folly::Future<folly::Unit> PrjfsDispatcherImpl::preSetHardlink(
    RelativePath relPath,
    RelativePath /*destPath*/,
    bool /*isDirectory*/,
    ObjectFetchContext& /*context*/) {
  return folly::makeFuture<folly::Unit>(makeHResultErrorExplicit(
      HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED),
      fmt::format(FMT_STRING("Hardlinks are not supported: {}"), relPath)));
}

} // namespace facebook::eden

#endif
