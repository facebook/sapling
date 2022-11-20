/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <cpptoml.h>
#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/executors/SerialExecutor.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/FileUtils.h"
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
  auto rootTable = cpptoml::make_table();
  auto configTable = cpptoml::make_table();
  configTable->insert(kConfigRootPath, mount.getPath().stringWithoutUNC());
  configTable->insert(
      kConfigSocketPath,
      mount.getServerState()->getSocketPath().stringWithoutUNC());
  configTable->insert(
      kConfigClientPath,
      mount.getCheckoutConfig()->getClientDirectory().stringWithoutUNC());
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
    const ObjectFetchContextPtr& context) {
  bool isRoot = path.empty();
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([isRoot,
                  objectStore = mount_->getObjectStore(),
                  context = context.copy()](
                     std::variant<std::shared_ptr<const Tree>, TreeEntry>
                         treeOrTreeEntry) mutable {
        auto& tree = std::get<std::shared_ptr<const Tree>>(treeOrTreeEntry);

        std::vector<PrjfsDirEntry> ret;
        ret.reserve(tree->size() + isRoot);
        for (const auto& treeEntry : *tree) {
          if (treeEntry.second.isTree()) {
            ret.emplace_back(
                treeEntry.first, true, ImmediateFuture<uint64_t>(0ull));
          } else {
            auto sizeFut =
                objectStore->getBlobSize(treeEntry.second.getHash(), context);
            ret.emplace_back(treeEntry.first, false, std::move(sizeFut));
          }
        }

        if (isRoot) {
          ret.emplace_back(
              kDotEdenPathComponent, true, ImmediateFuture<uint64_t>(0ull));
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
            } else {
              // An update to a commit not containing a directory but with
              // materialized and ignored subdirectories/files will still be
              // present in the working copy and will still be a placeholder
              // due to EdenFS not being able to make the directory full. We
              // thus simply return an empty directory and ProjectedFS will
              // combine it with the on-disk materialized state.
              return folly::Try{std::vector<PrjfsDirEntry>{}};
            }
          }
        }
        return dirEntries;
      });
}

ImmediateFuture<std::optional<LookupResult>> PrjfsDispatcherImpl::lookup(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue([this, context = context.copy(), path](
                     std::variant<std::shared_ptr<const Tree>, TreeEntry>
                         treeOrTreeEntry) mutable {
        bool isDir = std::holds_alternative<std::shared_ptr<const Tree>>(
            treeOrTreeEntry);
        auto pathFut = mount_->canonicalizePathFromTree(path, context);
        auto sizeFut = isDir
            ? ImmediateFuture<uint64_t>{0ull}
            : mount_->getObjectStore()->getBlobSize(
                  std::get<TreeEntry>(treeOrTreeEntry).getHash(), context);

        return collectAllSafe(pathFut, sizeFut)
            .thenValue([this, isDir, context = context.copy()](
                           std::tuple<RelativePath, uint64_t> res) {
              auto [path, size] = std::move(res);
              auto lookupResult = LookupResult{path, size, isDir};

              // We need to run the following asynchronously to avoid the risk
              // of deadlocks when EdenFS recursively triggers this lookup
              // call. In rare situation, this might happen during a checkout
              // operation which is already holding locks that the code below
              // also need.
              folly::via(
                  notificationExecutor_,
                  [&mount = *mount_,
                   path = std::move(path),
                   context = context.copy()]() {
                    // Finally, let's tell the TreeInode that this file needs
                    // invalidation during update. This is run in a separate
                    // executor to avoid deadlocks. This is guaranteed to 1) run
                    // before any other changes to this inode, and 2) before
                    // checkout starts invalidating files/directories.
                    // This also cannot race with a decFsRefcount from
                    // TreeInode::invalidateChannelEntryCache due to
                    // getInodeSlow needing to acquire the content lock that
                    // invalidateChannelEntryCache is already holding.
                    mount.getInodeSlow(path, context)
                        .thenValue(
                            [](InodePtr inode) { inode->incFsRefcount(); })
                        .get();
                  });

              return std::optional{std::move(lookupResult)};
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
                      std::move(path), dotEdenConfig_.length(), false}}};
                } else if (path == kDotEdenRelativePath) {
                  return folly::Try{
                      std::optional{LookupResult{std::move(path), 0, true}}};
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
    const ObjectFetchContextPtr& context) {
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
    const ObjectFetchContextPtr& context) {
  return mount_->getTreeOrTreeEntry(path, context)
      .thenValue(
          [context = context.copy(), objectStore = mount_->getObjectStore()](
              std::variant<std::shared_ptr<const Tree>, TreeEntry>
                  treeOrTreeEntry) {
            auto& treeEntry = std::get<TreeEntry>(treeOrTreeEntry);
            return objectStore->getBlob(treeEntry.getHash(), context)
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
    const ObjectFetchContextPtr& context) {
  auto treeInodeFut =
      mount.getInodeSlow(path, context).thenValue([](const InodePtr inode) {
        return inode.asTreePtr();
      });
  return std::move(treeInodeFut)
      .thenTry([path = std::move(path), &mount, context = context.copy()](
                   folly::Try<TreeInodePtr> result) {
        if (auto* exc = result.tryGetExceptionObject<std::system_error>();
            exc && isEnoent(*exc)) {
          mount.getStats()->increment(&PrjfsStats::outOfOrderCreate);
          XLOG_EVERY_MS(DBG2, 1000)
              << "Out of order directory creation notification for: " << path;

          /*
           * ProjectedFS notifications are asynchronous and sent after the
           * fact. This means that we can get a notification on a
           * file/directory before the parent directory notification has been
           * completed. This should be a very rare event and thus the code
           * below is pessimistic and will try to create all parent
           * directories.
           */

          auto fut = ImmediateFuture<TreeInodePtr>{mount.getRootInode()};
          for (auto parent : path.paths()) {
            fut = std::move(fut).thenValue(
                [parent = parent.copy(),
                 context = context.copy()](TreeInodePtr treeInode) {
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

enum class OnDiskState {
  MaterializedFile,
  MaterializedDirectory,
  NotPresent,
};

ImmediateFuture<OnDiskState> getOnDiskState(
    const EdenMount& mount,
    RelativePathPiece path,
    std::chrono::steady_clock::time_point receivedAt,
    int retry = 0) {
  auto absPath = mount.getPath() + path;
  auto boostPath = boost::filesystem::path(absPath.asString());

  boost::system::error_code ec;
  auto fileType = boost::filesystem::symlink_status(boostPath, ec).type();

  if (fileType == boost::filesystem::regular_file) {
    return OnDiskState::MaterializedFile;
  } else if (fileType == boost::filesystem::symlink_file) {
    return OnDiskState::MaterializedFile;
  } else if (fileType == boost::filesystem::reparse_file) {
    // Boost reports anything that is a reparse point which is not a symlink a
    // reparse_file. In particular, socket are reported as such.
    return OnDiskState::MaterializedFile;
  } else if (fileType == boost::filesystem::directory_file) {
    const auto elapsed = std::chrono::steady_clock::now() - receivedAt;
    const auto delay =
        mount.getEdenConfig()->prjfsDirectoryCreationDelay.getValue();
    if (elapsed < delay) {
      // See comment on EdenConfig::prjfsDirectoryCreationDelay for what's going
      // on here.
      auto timeToSleep =
          std::chrono::duration_cast<folly::HighResDuration>(delay - elapsed);
      return ImmediateFuture{folly::futures::sleep(timeToSleep)}.thenValue(
          [&mount, path = path.copy(), retry, receivedAt](folly::Unit&&) {
            return getOnDiskState(mount, path, receivedAt, retry);
          });
    }
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
        [&mount, path = path.copy(), receivedAt, retry](folly::Unit&&) {
          return getOnDiskState(mount, path, receivedAt, retry + 1);
        });
  } else {
    return makeImmediateFuture<OnDiskState>(std::logic_error(
        fmt::format("Unknown file type {} for file {}", fileType, path)));
  }
}

ImmediateFuture<folly::Unit> fileNotificationImpl(
    const EdenMount& mount,
    RelativePath path,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context);

ImmediateFuture<folly::Unit> handleNotPresentFileNotification(
    const EdenMount& mount,
    RelativePath path,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context) {
  /**
   * Allows finding the first directory that is not present on disk. This must
   * be heap allocated and kept alive until compute returns.
   */
  class GetFirstDirectoryNotPresent {
   public:
    explicit GetFirstDirectoryNotPresent(RelativePath path)
        : fullPath_{std::move(path)}, currentPrefix_{fullPath_} {}

    GetFirstDirectoryNotPresent(GetFirstDirectoryNotPresent&&) = delete;
    GetFirstDirectoryNotPresent(const GetFirstDirectoryNotPresent&) = delete;

    ImmediateFuture<RelativePath> compute(
        const EdenMount& mount,
        std::chrono::steady_clock::time_point receivedAt) {
      return getOnDiskState(mount, currentPrefix_.dirname(), receivedAt)
          .thenValue(
              [this, &mount, receivedAt](
                  OnDiskState state) mutable -> ImmediateFuture<RelativePath> {
                if (state == OnDiskState::MaterializedDirectory) {
                  return currentPrefix_.copy();
                }

                currentPrefix_ = currentPrefix_.dirname();
                return compute(mount, receivedAt);
              });
    }

   private:
    // The currentPrefix_ is a piece of the fullPath_ which is kept around for
    // lifetime reasons.
    RelativePath fullPath_;
    RelativePathPiece currentPrefix_;
  };

  // First, we need to figure out how far down this path has been removed.
  auto getFirstNotPresent =
      std::make_unique<GetFirstDirectoryNotPresent>(std::move(path));
  auto fut = getFirstNotPresent->compute(mount, receivedAt);
  return std::move(fut)
      .ensure([getFirstNotPresent = std::move(getFirstNotPresent)] {})
      .thenValue([&mount, context = context.copy(), receivedAt](
                     RelativePath path) {
        auto basename = path.basename();
        auto dirname = path.dirname();

        // Let's now remove the entire hierarchy.
        return createDirInode(mount, dirname.copy(), context)
            .thenValue([basename = basename.copy(), context = context.copy()](
                           const TreeInodePtr treeInode) {
              return treeInode->removeRecursively(
                  basename, InvalidationRequired::No, context);
            })
            .thenValue([&mount,
                        context = context.copy(),
                        path = std::move(path),
                        receivedAt](auto&&) mutable {
              // Now that the mismatch has been removed, make sure to also
              // trigger a notification on that path. A file might have been
              // created. Note that this may trigger a recursion into
              // handleNotPresentFileNotification, which will be caught by the
              // thenTry below due to the file/directory no longer being
              // present in the TreeInode.
              return fileNotificationImpl(
                  mount, std::move(path), receivedAt, context);
            })
            .thenTry([](folly::Try<folly::Unit> try_) {
              if (auto* exc = try_.tryGetExceptionObject<std::system_error>()) {
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
}

ImmediateFuture<folly::Unit> recursivelyUpdateChildrens(
    const EdenMount& mount,
    TreeInodePtr tree,
    RelativePath path,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context) {
  auto absPath = mount.getPath() + path;
  auto direntNamesTry = getAllDirectoryEntryNames(absPath);
  if (direntNamesTry.hasException()) {
    if (auto* exc = direntNamesTry.tryGetExceptionObject<std::system_error>()) {
      // In the case where the directory has been removed from the disk, we
      // should silently continue. A notification would have been sent to
      // EdenFS and will notice the directory missing then.
      if (isEnoent(*exc)) {
        return folly::unit;
      }
    }
    return makeImmediateFuture<folly::Unit>(direntNamesTry.exception());
  }
  const auto& direntNames = direntNamesTry.value();

  // To reduce the amount of disk activity, merge the filenames found on disk
  // with the ones in the inode.
  PathMap<folly::Unit> map{CaseSensitivity::Insensitive};
  {
    auto content = tree->getContents().rlock();
    map.reserve(direntNames.size() + content->entries.size());
    for (const auto& entry : content->entries) {
      map.emplace(entry.first, folly::unit);
    }
  }
  for (const auto& entry : direntNames) {
    map.emplace(entry, folly::unit);
  }

  std::vector<ImmediateFuture<folly::Unit>> futures;
  futures.reserve(map.size());

  // Now, trigger the recursive file notification to add/remove all the
  // files/directories to the inode.
  for (const auto& [entryName, unit] : map) {
    auto entryPath = path + entryName;
    futures.emplace_back(
        fileNotificationImpl(mount, std::move(entryPath), receivedAt, context));
  }

  return collectAllSafe(std::move(futures))
      .thenValue([](std::vector<folly::Unit>&&) { return folly::unit; });
}

ImmediateFuture<folly::Unit> handleMaterializedFileNotification(
    const EdenMount& mount,
    RelativePath path,
    InodeType inodeType,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context) {
  return createDirInode(mount, path.dirname().copy(), context)
      .thenValue([&mount,
                  path = std::move(path),
                  inodeType,
                  receivedAt,
                  context =
                      context.copy()](const TreeInodePtr treeInode) mutable {
        auto basename = path.basename();
        return treeInode->getOrLoadChild(basename, context)
            .thenTry(
                [&mount,
                 path = std::move(path),
                 treeInode,
                 inodeType,
                 receivedAt,
                 context = context.copy()](folly::Try<InodePtr> try_) mutable
                -> ImmediateFuture<folly::Unit> {
                  auto basename = path.basename();
                  if (try_.hasException()) {
                    if (auto* exc =
                            try_.tryGetExceptionObject<std::system_error>()) {
                      if (isEnoent(*exc)) {
                        if (inodeType == InodeType::TREE) {
                          auto child = treeInode->mkdir(
                              basename, _S_IFDIR, InvalidationRequired::No);
                          child->incFsRefcount();
                          return recursivelyUpdateChildrens(
                              mount,
                              std::move(child),
                              std::move(path),
                              receivedAt,
                              context);
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
                    case InodeType::TREE: {
                      if (auto inodePtr = inode.asTreePtrOrNull()) {
                        // In the case where this is already a directory, we
                        // still need to recursively add all the childrens.
                        // Consider the case where a directory is renamed and
                        // a file is added in it after it. If EdenFS handles
                        // the file creation prior to the renaming the
                        // directory will be created above in createDirInode,
                        // but we also need to make sure that all the files in
                        // the renamed directory are added too, hence the call
                        // to recursivelyAddAllChildrens.
                        return recursivelyUpdateChildrens(
                            mount,
                            std::move(inodePtr),
                            std::move(path),
                            receivedAt,
                            context);
                      }
                      // Somehow this is a file, but there is a directory on
                      // disk, let's remove it and create the directory.
                      return treeInode
                          ->unlink(basename, InvalidationRequired::No, context)
                          .thenTry([&mount,
                                    context = context.copy(),
                                    path = std::move(path),
                                    receivedAt,
                                    treeInode](
                                       folly::Try<folly::Unit> try_) mutable {
                            if (auto* exc = try_.tryGetExceptionObject<
                                            std::system_error>()) {
                              if (!isEnoent(*exc)) {
                                return makeImmediateFuture<folly::Unit>(
                                    try_.exception());
                              }
                            }
                            auto child = treeInode->mkdir(
                                path.basename(),
                                _S_IFDIR,
                                InvalidationRequired::No);
                            child->incFsRefcount();
                            return recursivelyUpdateChildrens(
                                mount,
                                std::move(child),
                                std::move(path),
                                receivedAt,
                                context);
                          });
                    }
                    case InodeType::FILE: {
                      if (auto fileInode = inode.asFilePtrOrNull()) {
                        fileInode->materialize();
                        return folly::unit;
                      }
                      // Somehow this is a directory, but there is a file on
                      // disk, let's remove it and create the file.
                      return treeInode
                          ->removeRecursively(
                              basename, InvalidationRequired::No, context)
                          .thenTry(
                              [basename = basename.copy(),
                               treeInode](folly::Try<folly::Unit> try_)
                                  -> ImmediateFuture<folly::Unit> {
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
                                return folly::unit;
                              });
                    }
                  }

                  return folly::unit;
                });
      });
}

ImmediateFuture<folly::Unit> fileNotificationImpl(
    const EdenMount& mount,
    RelativePath path,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context) {
  return getOnDiskState(mount, path, receivedAt)
      .thenValue([&mount,
                  path = std::move(path),
                  receivedAt,
                  context = context.copy()](OnDiskState state) mutable {
        switch (state) {
          case OnDiskState::MaterializedFile:
            return handleMaterializedFileNotification(
                mount, std::move(path), InodeType::FILE, receivedAt, context);
          case OnDiskState::MaterializedDirectory:
            return handleMaterializedFileNotification(
                mount, std::move(path), InodeType::TREE, receivedAt, context);
          case OnDiskState::NotPresent:
            return handleNotPresentFileNotification(
                mount, std::move(path), receivedAt, context);
        }
      });
}

ImmediateFuture<folly::Unit> fileNotification(
    EdenMount& mount,
    RelativePath path,
    folly::Executor::KeepAlive<folly::SequencedExecutor> executor,
    const ObjectFetchContextPtr& context) {
  auto receivedAt = std::chrono::steady_clock::now();
  folly::stop_watch<std::chrono::milliseconds> watch;

  folly::via(
      executor,
      [&mount, path, receivedAt, context = context.copy(), watch]() mutable {
        auto fault = ImmediateFuture{
            mount.getServerState()->getFaultInjector().checkAsync(
                "PrjfsDispatcherImpl::fileNotification", path)};

        std::move(fault)
            .thenValue([&mount,
                        path = std::move(path),
                        receivedAt,
                        context = context.copy()](auto&&) {
              return fileNotificationImpl(
                  mount, std::move(path), receivedAt, context);
            })
            .get();
        mount.getStats()->addDuration(
            &PrjfsStats::queuedFileNotification, watch.elapsed());
      })
      .thenError([path](const folly::exception_wrapper& ew) {
        // These should in theory never happen, but they sometimes happen
        // due to filesystem errors, antivirus scanning, etc. During
        // test, these should be treated as fatal errors, so we don't let
        // errors silently pass tests. In release builds, let's be less
        // aggressive and just log.
        XLOG(DFATAL) << "While handling notification on: " << path << ": "
                     << ew;
      });
  return folly::unit;
}

} // namespace

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileCreated(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirCreated(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileModified(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    const ObjectFetchContextPtr& context) {
  // A rename is just handled like 2 notifications separate notifications on
  // the old and new paths.
  auto oldNotification = fileNotification(
      *mount_, std::move(oldPath), notificationExecutor_, context);
  auto newNotification = fileNotification(
      *mount_, std::move(newPath), notificationExecutor_, context);

  return collectAllSafe(std::move(oldNotification), std::move(newNotification))
      .thenValue(
          [](std::tuple<folly::Unit, folly::Unit>&&) { return folly::unit; });
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preFileRename(
    RelativePath oldPath,
    RelativePath newPath,
    const ObjectFetchContextPtr& /*context*/) {
  if (oldPath == kDotEdenConfigPath || newPath == kDotEdenConfigPath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  if (newPath.dirname() == kDotEdenRelativePath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  return folly::unit;
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preDirRename(
    RelativePath oldPath,
    RelativePath newPath,
    const ObjectFetchContextPtr& /*context*/) {
  if (oldPath == kDotEdenRelativePath || newPath == kDotEdenRelativePath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  if (newPath.dirname() == kDotEdenRelativePath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  return folly::unit;
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileDeleted(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preFileDelete(
    RelativePath path,
    const ObjectFetchContextPtr& /*context*/) {
  if (path == kDotEdenConfigPath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  return folly::unit;
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirDeleted(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), notificationExecutor_, context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preDirDelete(
    RelativePath path,
    const ObjectFetchContextPtr& /*context*/) {
  if (path == kDotEdenRelativePath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }
  return folly::unit;
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
