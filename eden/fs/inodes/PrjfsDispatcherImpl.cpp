/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include <cpptoml.h>
#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/executors/SerialExecutor.h>
#include <folly/logging/xlog.h>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook::eden {

using namespace std::chrono_literals;

namespace {

const PathComponentPiece kDotEdenPathComponent{kDotEdenName};
const RelativePathPiece kDotEdenRelativePath{kDotEdenName};
const RelativePathPiece kDotEdenConfigPath{".eden/config"};
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
      executor_{1, "PrjfsDispatcher"},
      notificationExecutor_{
          folly::SerialExecutor::create(folly::getKeepAliveToken(&executor_))},
      dotEdenConfig_{makeDotEdenConfig(*mount)} {}

ImmediateFuture<std::vector<PrjfsDirEntry>> PrjfsDispatcherImpl::opendir(
    RelativePath path,
    ObjectFetchContext& context) {
  bool isRoot = path.empty();
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([isRoot, objectStore = mount_->getObjectStore()](
                     std::variant<std::shared_ptr<const Tree>, TreeEntry>
                         treeOrTreeEntry) {
        auto& tree = std::get<std::shared_ptr<const Tree>>(treeOrTreeEntry);
        auto& treeEntries = tree->getTreeEntries();

        std::vector<PrjfsDirEntry> ret;
        ret.reserve(treeEntries.size() + isRoot);
        for (const auto& treeEntry : treeEntries) {
          if (treeEntry.isTree()) {
            ret.emplace_back(
                treeEntry.getName(), true, ImmediateFuture<uint64_t>(0));
          } else {
            // Since the sizeFut may complete after the context is destroyed,
            // let's create a separate context.
            static ObjectFetchContext* sizeContext =
                ObjectFetchContext::getNullContextWithCauseDetail(
                    "PrjfsDispatcherImpl::opendir");
            auto sizeFut =
                objectStore->getBlobSize(treeEntry.getHash(), *sizeContext);
            ret.emplace_back(treeEntry.getName(), false, std::move(sizeFut));
          }
        }

        if (isRoot) {
          ret.emplace_back(
              kDotEdenPathComponent, true, ImmediateFuture<uint64_t>(0));
        }

        return ret;
      })
      .thenTry([this, path = std::move(path)](
                   folly::Try<std::vector<PrjfsDirEntry>> dirEntries) {
        if (auto* exc = dirEntries.tryGetExceptionObject<std::system_error>()) {
          if (isEnoent(*exc)) {
            if (path == kDotEdenRelativePath) {
              std::vector<PrjfsDirEntry> ret;
              ret.emplace_back(
                  PathComponent{kConfigTable},
                  false,
                  ImmediateFuture<uint64_t>(dotEdenConfig_.size()));
              return folly::Try{ret};
            }
          }
        }
        return dirEntries;
      });
}

ImmediateFuture<std::optional<LookupResult>> PrjfsDispatcherImpl::lookup(
    RelativePath path,
    ObjectFetchContext& context) {
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([this, &context, path](
                     std::variant<std::shared_ptr<const Tree>, TreeEntry>
                         treeOrTreeEntry) {
        bool isDir = std::holds_alternative<std::shared_ptr<const Tree>>(
            treeOrTreeEntry);
        auto pathFut = mount_->canonicalizePathFromTree(path, context);
        auto sizeFut = isDir
            ? ImmediateFuture<uint64_t>{0}
            : mount_->getObjectStore()->getBlobSize(
                  std::get<TreeEntry>(treeOrTreeEntry).getHash(), context);

        return collectAllSafe(pathFut, sizeFut)
            .thenValue([this, isDir, &context](
                           std::tuple<RelativePath, uint64_t> res) {
              auto [path, size] = std::move(res);
              auto inodeMetadata = InodeMetadata{path, size, isDir};

              // Finally, let's tell the TreeInode that this file needs
              // invalidation during update.
              return mount_->getInode(path, context)
                  .thenValue([inodeMetadata =
                                  std::move(inodeMetadata)](InodePtr inode) {
                    // Since a lookup is needed prior to any file operation,
                    // this getInode call shouldn't race with concurrent file
                    // removal/move
                    return std::optional{LookupResult{
                        std::move(inodeMetadata), [inode = std::move(inode)] {
                          inode->incFsRefcount();
                        }}};
                  });
            });
      })
      .thenTry(
          [this, path = std::move(path)](
              folly::Try<std::optional<LookupResult>> result)
              -> folly::Try<std::optional<LookupResult>> {
            if (auto* exc = result.tryGetExceptionObject<std::system_error>()) {
              if (isEnoent(*exc)) {
                if (path == kDotEdenConfigPath) {
                  return folly::Try{std::optional{LookupResult{
                      InodeMetadata{
                          std::move(path), dotEdenConfig_.length(), false},
                      [] {}}}};
                } else if (path == kDotEdenRelativePath) {
                  return folly::Try{std::optional{LookupResult{
                      InodeMetadata{std::move(path), 0, true}, [] {}}}};
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
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([](auto&&) { return true; })
      .thenTry([path = std::move(path)](folly::Try<bool> result) {
        if (auto* exc = result.tryGetExceptionObject<std::system_error>()) {
          if (isEnoent(*exc)) {
            if (path == kDotEdenRelativePath || path == kDotEdenConfigPath) {
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
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([&context, objectStore = mount_->getObjectStore()](
                     std::variant<std::shared_ptr<const Tree>, TreeEntry>
                         treeOrTreeEntry) {
        auto& treeEntry = std::get<TreeEntry>(treeOrTreeEntry);
        return ImmediateFuture{
            objectStore->getBlob(treeEntry.getHash(), context).semi()}
            .thenValue([](std::shared_ptr<const Blob> blob) {
              // TODO(xavierd): directly return the Blob to the caller.
              std::string res;
              blob->getContents().appendTo(res);
              return res;
            });
      })
      .thenTry([this, path = std::move(path)](folly::Try<std::string> result) {
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

enum class OnDiskState {
  MaterializedFile,
  MaterializedDirectory,
  NotPresent,
};

ImmediateFuture<OnDiskState>
getOnDiskState(const EdenMount& mount, RelativePathPiece path, int retry = 0) {
  auto absPath = mount.getPath() + path;
  auto boostPath = boost::filesystem::path(absPath.stringPiece());

  boost::system::error_code ec;
  auto fileType = boost::filesystem::status(boostPath, ec).type();

  if (fileType == boost::filesystem::regular_file) {
    return OnDiskState::MaterializedFile;
  } else if (fileType == boost::filesystem::directory_file) {
    return OnDiskState::MaterializedDirectory;
  } else if (fileType == boost::filesystem::file_not_found) {
    return OnDiskState::NotPresent;
  } else if (fileType == boost::filesystem::status_error) {
    if (retry == 5) {
      XLOG(WARN) << "Assuming path is not present: " << path;
      return OnDiskState::NotPresent;
    }
    XLOG(WARN) << "Error: " << ec.message() << " for path: " << path;
    return ImmediateFuture{folly::futures::sleep(retry * 5ms)}.thenValue(
        [&mount, path, retry](folly::Unit&&) {
          return getOnDiskState(mount, path, retry + 1);
        });
  } else {
    return makeImmediateFuture<OnDiskState>(std::logic_error(
        fmt::format("Unknown file type {} for file {}", fileType, path)));
  }
}

ImmediateFuture<folly::Unit> handleMaterializedFileNotification(
    EdenMount& mount,
    RelativePath path,
    InodeType inodeType,
    ObjectFetchContext& context) {
  return createDirInode(mount, path.dirname().copy(), context)
      .thenValue([basename = path.basename().copy(), inodeType, &context](
                     const TreeInodePtr treeInode) mutable {
        return treeInode->getOrLoadChild(basename, context)
            .thenTry(
                [basename = std::move(basename),
                 treeInode,
                 inodeType,
                 &context](folly::Try<InodePtr> try_) mutable
                -> ImmediateFuture<folly::Unit> {
                  if (try_.hasException()) {
                    if (auto* exc =
                            try_.tryGetExceptionObject<std::system_error>()) {
                      if (isEnoent(*exc)) {
                        if (inodeType == InodeType::Tree) {
                          auto child = treeInode->mkdir(
                              basename, _S_IFDIR, InvalidationRequired::No);
                          child->incFsRefcount();
                        } else {
                          auto child = treeInode->mknod(
                              basename, _S_IFREG, 0, InvalidationRequired::No);
                          child->incFsRefcount();
                        }
                        return folly::unit;
                      }
                    }
                    return makeImmediateFuture<folly::Unit>(try_.exception());
                  }

                  auto inode = std::move(try_).value();
                  switch (inodeType) {
                    case InodeType::Tree: {
                      if (inode.asTreePtrOrNull()) {
                        return folly::unit;
                      }
                      // Somehow this is a file, but there is a directory on
                      // disk, let's remove it and create the directory.
                      return treeInode
                          ->unlink(basename, InvalidationRequired::No, context)
                          .thenTry([basename = std::move(basename),
                                    treeInode](folly::Try<folly::Unit> try_) {
                            if (auto* exc = try_.tryGetExceptionObject<
                                            std::system_error>()) {
                              if (!isEnoent(*exc)) {
                                return makeImmediateFuture<folly::Unit>(
                                    try_.exception());
                              }
                            }
                            auto child = treeInode->mkdir(
                                basename, _S_IFDIR, InvalidationRequired::No);
                            child->incFsRefcount();
                            return ImmediateFuture{folly::unit};
                          });
                    }
                    case InodeType::File: {
                      if (auto fileInode = inode.asFilePtrOrNull()) {
                        fileInode->materialize();
                        return folly::unit;
                      }
                      // Somehow this is a directory, but there is a file on
                      // disk, let's remove it and create the file.
                      return treeInode
                          ->removeRecursively(
                              basename, InvalidationRequired::No, context)
                          .thenTry([basename = std::move(basename),
                                    treeInode](folly::Try<folly::Unit> try_) {
                            if (auto* exc = try_.tryGetExceptionObject<
                                            std::system_error>()) {
                              if (!isEnoent(*exc)) {
                                return makeImmediateFuture<folly::Unit>(
                                    try_.exception());
                              }
                            }
                            auto child = treeInode->mknod(
                                basename,
                                _S_IFREG,
                                0,
                                InvalidationRequired::No);
                            child->incFsRefcount();
                            return ImmediateFuture{folly::unit};
                          });
                    }
                  }

                  return folly::unit;
                });
      });
}

ImmediateFuture<folly::Unit> handleNotPresentFileNotification(
    EdenMount& mount,
    RelativePath path,
    ObjectFetchContext& context) {
  struct GetFirstPresent {
    GetFirstPresent() = default;

    ImmediateFuture<RelativePathPiece> compute(
        EdenMount& mount,
        RelativePathPiece path) {
      auto dirname = path.dirname();
      return getOnDiskState(mount, dirname)
          .thenValue(
              [this, &mount, path](
                  OnDiskState state) -> ImmediateFuture<RelativePathPiece> {
                if (state != OnDiskState::NotPresent) {
                  return path;
                }

                return compute(mount, path.dirname());
              });
    }
  };

  // First, we need to figure out how far down this path has been removed.
  return GetFirstPresent{}
      .compute(mount, path)
      .thenValue([&mount, &context](RelativePathPiece path) {
        auto basename = path.basename();
        auto dirname = path.dirname();

        // Let's now remove the entire hierarchy.
        return createDirInode(mount, dirname.copy(), context)
            .thenValue([basename = basename.copy(),
                        &context](const TreeInodePtr treeInode) {
              return treeInode
                  ->removeRecursively(
                      basename, InvalidationRequired::No, context)
                  .thenTry([](folly::Try<folly::Unit> try_) {
                    if (auto* exc =
                            try_.tryGetExceptionObject<std::system_error>()) {
                      if (isEnoent(*exc)) {
                        // ProjectedFS can sometimes send multiple deletion
                        // notification for the same file, in which case a
                        // previous deletion will have removed the file already.
                        // We can safely ignore the error here.
                        return folly::Try{folly::unit};
                      }
                    }
                    return try_;
                  });
            });
      })
      .ensure([path = std::move(path)] {});
}

ImmediateFuture<folly::Unit> fileNotification(
    EdenMount& mount,
    RelativePath path,
    folly::Executor::KeepAlive<folly::SequencedExecutor> executor,
    ObjectFetchContext& context) {
  folly::via(executor, [&mount, path, &context]() mutable {
    return getOnDiskState(mount, path)
        .thenValue([&mount, path = std::move(path), &context](
                       OnDiskState state) mutable {
          switch (state) {
            case OnDiskState::MaterializedFile:
              return handleMaterializedFileNotification(
                  mount, std::move(path), InodeType::File, context);
            case OnDiskState::MaterializedDirectory:
              return handleMaterializedFileNotification(
                  mount, std::move(path), InodeType::Tree, context);
            case OnDiskState::NotPresent:
              return handleNotPresentFileNotification(
                  mount, std::move(path), context);
          }
        })
        .get();
  }).thenError([path](const folly::exception_wrapper& ew) {
    XLOG(ERR) << "While handling notification on: " << path << ": " << ew;
  });
  return folly::unit;
}

} // namespace

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileCreated(
    RelativePath path,
    ObjectFetchContext& context) {
  return fileNotification(*mount_, path, notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirCreated(
    RelativePath path,
    ObjectFetchContext& context) {
  return fileNotification(*mount_, path, notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileModified(
    RelativePath path,
    ObjectFetchContext& context) {
  return fileNotification(*mount_, path, notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    ObjectFetchContext& context) {
  // A rename is just handled like 2 notifications separate notifications on
  // the old and new paths.
  auto oldNotification =
      fileNotification(*mount_, oldPath, notificationExecutor_, context);
  auto newNotification =
      fileNotification(*mount_, newPath, notificationExecutor_, context);

  return collectAllSafe(std::move(oldNotification), std::move(newNotification))
      .thenValue(
          [](std::tuple<folly::Unit, folly::Unit>&&) { return folly::unit; });
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileDeleted(
    RelativePath path,
    ObjectFetchContext& context) {
  return fileNotification(*mount_, path, notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirDeleted(
    RelativePath path,
    ObjectFetchContext& context) {
  return fileNotification(*mount_, path, notificationExecutor_, context);
}

ImmediateFuture<folly::Unit>
PrjfsDispatcherImpl::waitForPendingNotifications() {
  // Since the executor is a SequencedExecutor, and the fileNotification
  // function blocks in the executor, the body of the lambda will only be
  // executed when all previously enqueued notifications have completed.
  //
  // Note that this synchronization only guarantees that writes from a the
  // calling application thread have completed when the future complete. Writes
  // made by a concurrent process or a different thread may still be in
  // ProjectedFS queue and therefore may still be pending when the future
  // complete. This is expected and therefore not a bug.
  return ImmediateFuture{
      folly::via(notificationExecutor_, []() { return folly::unit; }).semi()};
}

} // namespace facebook::eden

#endif
