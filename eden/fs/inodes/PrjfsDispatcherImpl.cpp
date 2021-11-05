/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include <cpptoml.h>
#include <folly/executors/QueuedImmediateExecutor.h>
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
  auto clientPath = mount.getCheckoutConfig()->getClientDirectory();

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

ImmediateFuture<std::vector<PrjfsDirEntry>> PrjfsDispatcherImpl::opendir(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context).thenValue([](const InodePtr inode) {
    auto treePtr = inode.asTreePtr();
    return treePtr->readdir();
  });
}

ImmediateFuture<std::optional<LookupResult>> PrjfsDispatcherImpl::lookup(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue(
          [&context](const InodePtr inode) mutable
          -> ImmediateFuture<std::optional<LookupResult>> {
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
                  return std::optional{LookupResult{
                      std::move(inodeMetadata), std::move(incFsRefcount)}};
                });
          })
      .thenTry(
          [path = std::move(path),
           this](folly::Try<std::optional<LookupResult>> result) mutable
          -> folly::Try<std::optional<LookupResult>> {
            if (auto* exc = result.tryGetExceptionObject<std::system_error>()) {
              if (isEnoent(*exc)) {
                if (path == kDotEdenConfigPath) {
                  return folly::Try{std::optional{LookupResult{
                      InodeMetadata{
                          std::move(path), dotEdenConfig_.length(), false},
                      [] {}}}};
                } else {
                  XLOG(DBG6) << path << ": File not found";
                  return folly::Try<std::optional<LookupResult>>{std::nullopt};
                }
              }
            }
            return result;
          });
}

ImmediateFuture<bool> PrjfsDispatcherImpl::access(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue([](const InodePtr) { return true; })
      .thenTry([path = std::move(path)](folly::Try<bool> result) {
        if (auto* exc = result.tryGetExceptionObject<std::system_error>()) {
          if (isEnoent(*exc)) {
            if (path == kDotEdenConfigPath) {
              return folly::Try<bool>{true};
            } else {
              return folly::Try<bool>{false};
            }
          }
        }
        return result;
      });
}

ImmediateFuture<std::string> PrjfsDispatcherImpl::read(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context)
      .thenValue([&context](const InodePtr inode) {
        auto fileInode = inode.asFilePtr();
        return fileInode->readAll(context).semi();
      })
      .thenTry([path = std::move(path), this](folly::Try<std::string> result) {
        if (auto* exc = result.tryGetExceptionObject<std::system_error>()) {
          if (isEnoent(*exc) && path == kDotEdenConfigPath) {
            return folly::Try<std::string>{std::string(dotEdenConfig_)};
          }
        }
        return result;
      });
}

namespace {
ImmediateFuture<TreeInodePtr> createDirInode(
    const EdenMount& mount,
    RelativePath path,
    ObjectFetchContext& context) {
  auto treeInodeFut =
      mount.getInode(path, context).thenValue([](const InodePtr inode) {
        return inode.asTreePtr();
      });
  return std::move(treeInodeFut)
      .thenTry([path = std::move(path), &mount, &context](
                   folly::Try<TreeInodePtr> result) {
        if (auto* exc = result.tryGetExceptionObject<std::system_error>();
            exc && isEnoent(*exc)) {
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

          auto fut = ImmediateFuture{mount.getRootInode()};
          for (auto parent : path.paths()) {
            fut = std::move(fut).thenValue(
                [parent = parent.copy(), &context](TreeInodePtr treeInode) {
                  auto basename = parent.basename();
                  try {
                    auto inode = treeInode->mkdir(
                        basename, _S_IFDIR, InvalidationRequired::No);
                    inode->incFsRefcount();
                  } catch (const std::system_error& ex) {
                    if (ex.code().value() != EEXIST) {
                      throw;
                    }
                  }

                  return treeInode->getOrLoadChildTree(basename, context);
                });
          }

          return fut;
        }
        return ImmediateFuture<TreeInodePtr>{std::move(result)};
      });
}

enum class InodeType : bool {
  Tree,
  File,
};

ImmediateFuture<folly::Unit> createInode(
    const EdenMount& mount,
    RelativePath path,
    InodeType inodeType,
    ObjectFetchContext& context) {
  auto treeInodeFut = createDirInode(mount, path.dirname().copy(), context);
  return std::move(treeInodeFut)
      .thenValue(
          [path = std::move(path), inodeType](const TreeInodePtr treeInode) {
            if (inodeType == InodeType::Tree) {
              try {
                auto inode = treeInode->mkdir(
                    path.basename(), _S_IFDIR, InvalidationRequired::No);
                inode->incFsRefcount();
              } catch (const std::system_error& ex) {
                /*
                 * If a concurrent createFile for a child of this directory
                 * finished before this one, the directory will already exist.
                 * This is not an error.
                 */
                if (ex.code().value() != EEXIST) {
                  return folly::Try<folly::Unit>(ex);
                }
              }
            } else {
              auto inode = treeInode->mknod(
                  path.basename(), _S_IFREG, 0, InvalidationRequired::No);
              inode->incFsRefcount();
            }

            return folly::Try{folly::unit};
          });
}

ImmediateFuture<folly::Unit> removeInode(
    const EdenMount& mount,
    RelativePath path,
    InodeType inodeType,
    ObjectFetchContext& context) {
  auto inodeFut = mount.getInode(path.dirname(), context);
  return std::move(inodeFut).thenValue(
      [path = std::move(path), inodeType, &context](const InodePtr inode) {
        auto treeInodePtr = inode.asTreePtr();
        if (inodeType == InodeType::Tree) {
          return treeInodePtr->rmdir(
              path.basename(), InvalidationRequired::No, context);
        } else {
          return treeInodePtr->unlink(
              path.basename(), InvalidationRequired::No, context);
        }
      });
}

} // namespace

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileCreated(
    RelativePath path,
    ObjectFetchContext& context) {
  return createInode(*mount_, std::move(path), InodeType::File, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirCreated(
    RelativePath path,
    ObjectFetchContext& context) {
  return createInode(*mount_, std::move(path), InodeType::Tree, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileModified(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getInode(path, context).thenValue([](const InodePtr inode) {
    auto fileInode = inode.asFilePtr();
    fileInode->materialize();
    return folly::unit;
  });
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    ObjectFetchContext& context) {
  auto oldParentInode =
      createDirInode(*mount_, oldPath.dirname().copy(), context);
  auto newParentInode =
      createDirInode(*mount_, newPath.dirname().copy(), context);

  return collectAllSafe(std::move(oldParentInode), std::move(newParentInode))
      .thenValue(
          [oldPath = std::move(oldPath),
           newPath = std::move(newPath),
           &context](const std::tuple<TreeInodePtr, TreeInodePtr> inodes) {
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
                InvalidationRequired::No,
                context);
          });
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileDeleted(
    RelativePath path,
    ObjectFetchContext& context) {
  return removeInode(*mount_, std::move(path), InodeType::File, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirDeleted(
    RelativePath path,
    ObjectFetchContext& context) {
  return removeInode(*mount_, std::move(path), InodeType::Tree, context);
}

} // namespace facebook::eden

#endif
