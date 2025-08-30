/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32
#include "eden/fs/inodes/PrjfsDispatcherImpl.h"
#include <filesystem>

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <cpptoml.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <optional>

#include "eden/common/telemetry/StructuredLogger.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/SystemError.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/LogEvent.h"

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
    : PrjfsDispatcher(mount->getStats().copy()),
      mount_{mount},
      dotEdenConfig_{makeDotEdenConfig(*mount)},
      symlinksEnabled_{
          mount_->getCheckoutConfig()->getEnableWindowsSymlinks()} {}

EdenTimestamp PrjfsDispatcherImpl::getLastCheckoutTime() const {
  return mount_->getLastCheckoutTime();
}

ImmediateFuture<std::vector<PrjfsDirEntry>> PrjfsDispatcherImpl::opendir(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return mount_->getServerState()
      ->getFaultInjector()
      .checkAsync("PrjfsDispatcherImpl::opendir", path.view())
      .thenValue([this, path = std::move(path), context = context.copy()](
                     auto&&) mutable {
        bool isRoot = path.empty();
        return mount_->getTreeOrTreeEntry(path, context)
            .thenValue([this,
                        path,
                        isRoot,
                        objectStore = mount_->getObjectStore(),
                        symlinksSupported = mount_->getCheckoutConfig()
                                                ->getEnableWindowsSymlinks(),
                        context = context.copy()](
                           std::variant<std::shared_ptr<const Tree>, TreeEntry>
                               treeOrTreeEntry) mutable {
              auto& tree =
                  std::get<std::shared_ptr<const Tree>>(treeOrTreeEntry);

              std::vector<PrjfsDirEntry> ret;
              ret.reserve(tree->size() + isRoot);
              for (const auto& treeEntry : *tree) {
                if (treeEntry.second.isTree()) {
                  ret.emplace_back(
                      treeEntry.first,
                      true,
                      std::nullopt,
                      ImmediateFuture<uint64_t>(0ull));
                } else {
                  auto optSymlinkTargetFut =
                      (symlinksSupported &&
                       treeEntry.second.getDtype() == dtype_t::Symlink)
                      ? std::make_optional(
                            objectStore
                                ->getBlob(
                                    treeEntry.second.getObjectId(),
                                    context.copy())
                                .thenValue(
                                    [this,
                                     name = treeEntry.first,
                                     path,
                                     context = context.copy()](
                                        std::shared_ptr<const Blob> blob) {
                                      auto content = blob->asString();
                                      std::replace(
                                          content.begin(),
                                          content.end(),
                                          '/',
                                          '\\');
                                      auto symlinkPath = path.empty()
                                          ? RelativePath(name)
                                          : path + name;
                                      return isFinalSymlinkPathDirectory(
                                                 symlinkPath, content, context)
                                          .thenValue(
                                              [content = std::move(content)](
                                                  bool isDir) {
                                                return std::make_pair(
                                                    content, isDir);
                                              });
                                    }))
                      : std::nullopt;
                  ret.emplace_back(
                      treeEntry.first,
                      false,
                      std::move(optSymlinkTargetFut),
                      objectStore->getBlobSize(
                          treeEntry.second.getObjectId(), context.copy()));
                }
              }

              if (isRoot) {
                ret.emplace_back(
                    kDotEdenPathComponent,
                    true,
                    std::nullopt,
                    ImmediateFuture<uint64_t>(0ull));
              }

              return ret;
            })
            .thenTry([this, path = std::move(path)](
                         folly::Try<std::vector<PrjfsDirEntry>> dirEntries) {
              if (auto* exc =
                      dirEntries.tryGetExceptionObject<std::system_error>()) {
                if (isEnoent(*exc)) {
                  if (path == kDotEdenRelativePath) {
                    std::vector<PrjfsDirEntry> ret;
                    ret.emplace_back(
                        PathComponent{kConfigTable},
                        false,
                        std::nullopt,
                        ImmediateFuture<uint64_t>(dotEdenConfig_.size()));
                    return folly::Try{ret};
                  } else {
                    // An update to a commit not containing a directory but with
                    // materialized and ignored subdirectories/files will still
                    // be present in the working copy and will still be a
                    // placeholder due to EdenFS not being able to make the
                    // directory full. We thus simply return an empty directory
                    // and ProjectedFS will combine it with the on-disk
                    // materialized state.
                    return folly::Try{std::vector<PrjfsDirEntry>{}};
                  }
                }
              }
              return dirEntries;
            });
      });
}

namespace {
bool isNonEdenFsPathDirectory(AbsolutePath path) {
  // TODO(sggutier): This might actually be another EdenFS repo instead of a
  // regular file. We should try to consider the case where the other EdenFS
  // repo in turn points out to somewhere inside of the EdenFS repo that
  // initiated this call, as trying to recursively resolve symlinks on this
  // manner might cause issues.
  boost::system::error_code ec;
  auto boostPath = boost::filesystem::path(path.asString());
  auto fileType = boost::filesystem::status(boostPath, ec).type();
  return fileType == boost::filesystem::directory_file;
}
} // namespace

std::variant<AbsolutePath, RelativePath>
PrjfsDispatcherImpl::determineTargetType(
    RelativePath symlink,
    string_view targetStringView) {
  // Creating absolute path symlinks with a variety of tools (e.g.,
  // mklink on Windows or os.symlink on Python) makes the created
  // symlinks start with an UNC prefix. However, there could be tools
  // that create symlinks that don't add this prefix.
  // TODO: Make this line also consider tools that do not add an UNC
  // prefix to absolute path symlinks.
  auto targetString = targetStringView.starts_with(detail::kUNCPrefix)
      ? std::string(targetStringView)
      : fmt::format(
            "{}{}{}",
            mount_->getPath() + symlink.dirname(),
            kDirSeparatorStr,
            targetStringView);
  AbsolutePath absTarget;
  try {
    absTarget = canonicalPath(targetString);
  } catch (const std::exception& exc) {
    XLOGF(
        DBG6,
        "unable to normalize target {}: {}",
        symlink.asString(),
        exc.what());
    throw exc;
  }
  RelativePath target;
  try {
    // Symlink points inside of EdenFS
    return RelativePath(mount_->getPath().relativize(absTarget));
  } catch (const std::exception&) {
    // Symlink points outside of EdenFS
    return absTarget;
  }
}

ImmediateFuture<std::variant<AbsolutePath, RelativePath>>
PrjfsDispatcherImpl::resolveSymlinkPath(
    RelativePath path,
    const ObjectFetchContextPtr& context,
    const size_t remainingRecursionDepth) {
  std::vector<RelativePath> pathParts;
  std::transform(
      path.paths().begin(),
      path.paths().end(),
      std::back_inserter(pathParts),
      [](const auto& p) { return RelativePath(p); });
  return resolveSymlinkPathImpl(
      std::move(path),
      context,
      std::move(pathParts),
      0,
      remainingRecursionDepth);
}

ImmediateFuture<std::variant<AbsolutePath, RelativePath>>
PrjfsDispatcherImpl::resolveSymlinkPathImpl(
    RelativePath path,
    const ObjectFetchContextPtr& context,
    std::vector<RelativePath> pathParts,
    const size_t solvedLen,
    const size_t remainingRecursionDepth) {
  if (solvedLen >= pathParts.size() || remainingRecursionDepth == 0) {
    // Either everything is resolved or we should give up due to recursion depth
    return std::move(path);
  }
  RelativePath target = pathParts[solvedLen];
  return mount_->getTreeOrTreeEntry(target, context)
      .thenValue(
          [this,
           path = path.copy(),
           symlink = std::move(target),
           context = context.copy(),
           pathParts = std::move(pathParts),
           solvedLen,
           remainingRecursionDepth](
              std::variant<std::shared_ptr<const Tree>, TreeEntry>
                  treeOrTreeEntry) mutable
          -> ImmediateFuture<std::variant<AbsolutePath, RelativePath>> {
            if (std::holds_alternative<std::shared_ptr<const Tree>>(
                    treeOrTreeEntry)) {
              // Everything up to the current component is a directory and ok,
              // keep normalizing the rest of the path
              return resolveSymlinkPathImpl(
                  std::move(path),
                  context,
                  std::move(pathParts),
                  solvedLen + 1,
                  remainingRecursionDepth);
            }
            auto& entry = std::get<TreeEntry>(treeOrTreeEntry);
            if (entry.getDtype() != dtype_t::Symlink) {
              // Some part of the path is a file; it does not make sense to keep
              // trying to resolve the rest
              return std::move(path);
            }
            return mount_->getObjectStore()
                ->getBlob(entry.getObjectId(), context)
                .thenValue(
                    [this,
                     context = context.copy(),
                     symlink = std::move(symlink),
                     path = std::move(path),
                     pathParts = std::move(pathParts),
                     solvedLen,
                     remainingRecursionDepth](
                        std::shared_ptr<const Blob> blob) mutable
                    -> ImmediateFuture<
                        std::variant<AbsolutePath, RelativePath>> {
                      // Resolve the symlink at this point and replace it in the
                      // path, then keep normalizing
                      auto content = blob->asString();
                      std::replace(content.begin(), content.end(), '/', '\\');
                      std::variant<AbsolutePath, RelativePath> resolvedTarget;
                      try {
                        resolvedTarget = determineTargetType(symlink, content);
                      } catch (const std::exception&) {
                        // The symlink target is invalid, just give up
                        return std::move(path);
                      }
                      std::optional<RelativePath> remainingPath = std::nullopt;
                      if (solvedLen != pathParts.size() - 1) {
                        // Even after partially resolving a symlink in the path,
                        // it's possible that we have a remainder in the path
                        // that needs to be attached to it. For instance, if we
                        // are resolving a path like a/b/c/x/y/z, c is a symlink
                        // to ../w, and the rest are regular directories then
                        // after replacing c by its symlink, resolvedTarget
                        // would be a/w . However, we still need to attach x/y/z
                        // to it. In this case, remainingPath would be x/y/z.
                        std::vector<RelativePathPiece> suffixes(
                            path.rsuffixes().begin(), path.rsuffixes().end());
                        remainingPath = RelativePath(
                            suffixes[pathParts.size() - solvedLen - 2]);
                      }
                      if (std::holds_alternative<AbsolutePath>(
                              resolvedTarget)) {
                        // The symlink target is absolute, but we are resolving
                        // a relative path. This means that the symlink target
                        // is outside of EdenFS. In this case, we can only
                        // return the absolute path.
                        auto absPath = std::get<AbsolutePath>(resolvedTarget);
                        if (remainingPath.has_value()) {
                          absPath = absPath + remainingPath.value();
                        }
                        return absPath;
                      }
                      auto newPath = std::get<RelativePath>(resolvedTarget);
                      if (remainingPath.has_value()) {
                        newPath = newPath + remainingPath.value();
                      }
                      // We need to rebuild the paths here, so we don't pass
                      // pathParts. Also, we cannot make assumptions about the
                      // position we are in as canonicalizing the path might
                      // have set us back so we don't pass solvedLen either
                      return resolveSymlinkPath(
                          std::move(newPath),
                          context,
                          remainingRecursionDepth - 1);
                    });
          })
      .thenError(
          [path = path.copy()](const folly::exception_wrapper&)
              -> ImmediateFuture<std::variant<AbsolutePath, RelativePath>> {
            // Something is wrong in the path, stop caring and return the entire
            // path
            return std::move(path);
          });
}

ImmediateFuture<bool> PrjfsDispatcherImpl::isFinalSymlinkPathDirectory(
    RelativePath symlink,
    string_view targetStringView,
    const ObjectFetchContextPtr& context,
    const int remainingRecursionDepth) {
  if (remainingRecursionDepth == 0) {
    return false;
  }

  // If the file starts with a "/", assume it's an absolute POSIX path and
  // refuse to resolve it.
  if (!targetStringView.starts_with(detail::kUNCPrefix) &&
      targetStringView.starts_with("\\")) {
    return false;
  }

  bool newCheck = true;
  {
    // We need to mark symlinks as visited to avoid infinite loops.
    auto sptr = symlinkCheck_.wlock();
    auto rs = sptr->emplace(symlink);
    newCheck = rs.second;
  }
  if (!newCheck) {
    return false;
  }

  return makeImmediateFutureWith([&]() -> ImmediateFuture<bool> {
           RelativePath target;
           std::variant<AbsolutePath, RelativePath> resolvedTarget;
           try {
             resolvedTarget = determineTargetType(symlink, targetStringView);
           } catch (const std::exception&) {
             return false;
           }
           if (std::holds_alternative<RelativePath>(resolvedTarget)) {
             target = std::get<RelativePath>(resolvedTarget);
           } else {
             // Symlink points outside of EdenFS; make the system solve it for
             // us
             return isNonEdenFsPathDirectory(
                 std::get<AbsolutePath>(resolvedTarget));
           }
           // This recursively goes through symlinks until it gets the first
           // entry that is not a symlink. Symlink cycles are prevented by the
           // check above.
           return resolveSymlinkPath(target, context)
               .thenValue(
                   [this, remainingRecursionDepth, context = context.copy()](
                       std::variant<AbsolutePath, RelativePath> resolvedTarget)
                       -> ImmediateFuture<bool> {
                     if (std::holds_alternative<AbsolutePath>(resolvedTarget)) {
                       return isNonEdenFsPathDirectory(
                           std::get<AbsolutePath>(resolvedTarget));
                     }
                     RelativePath target =
                         std::get<RelativePath>(resolvedTarget);
                     return mount_->getTreeOrTreeEntry(target, context)
                         .thenValue(
                             [this,
                              target = std::move(target),
                              context = context.copy(),
                              remainingRecursionDepth](
                                 std::variant<
                                     std::shared_ptr<const Tree>,
                                     TreeEntry> treeOrTreeEntry) mutable
                             -> ImmediateFuture<bool> {
                               if (std::holds_alternative<
                                       std::shared_ptr<const Tree>>(
                                       treeOrTreeEntry)) {
                                 return true;
                               }
                               auto entry =
                                   std::get<TreeEntry>(treeOrTreeEntry);
                               if (entry.getDtype() != dtype_t::Symlink) {
                                 return false;
                               }
                               return mount_->getObjectStore()
                                   ->getBlob(entry.getObjectId(), context)
                                   .thenValue([this,
                                               context = context.copy(),
                                               path = std::move(target),
                                               remainingRecursionDepth](
                                                  std::shared_ptr<const Blob>
                                                      blob) mutable {
                                     auto content = blob->asString();
                                     return isFinalSymlinkPathDirectory(
                                         std::move(path),
                                         content,
                                         context,
                                         remainingRecursionDepth - 1);
                                   });
                             });
                   })
               .thenError(
                   [](const folly::exception_wrapper&) { return false; });
         })
      .ensure([this, symlink] {
        auto sptr = symlinkCheck_.wlock();
        sptr->erase(symlink);
      });
}

ImmediateFuture<std::optional<LookupResult>> PrjfsDispatcherImpl::lookup(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return mount_->getServerState()
      ->getFaultInjector()
      .checkAsync("PrjfsDispatcherImpl::lookup", path.view())
      .thenValue([this, path = std::move(path), context = context.copy()](
                     auto&&) mutable {
        return mount_->getTreeOrTreeEntry(path, context)
            .thenValue([this, context = context.copy(), path](
                           std::variant<std::shared_ptr<const Tree>, TreeEntry>
                               treeOrTreeEntry) mutable {
              bool isDir = std::holds_alternative<std::shared_ptr<const Tree>>(
                  treeOrTreeEntry);
              auto pathFut = mount_->canonicalizePathFromTree(path, context);
              auto treeEntry = isDir
                  ? std::nullopt
                  : std::make_optional(std::get<TreeEntry>(treeOrTreeEntry));
              auto sizeFut = isDir ? ImmediateFuture<uint64_t>{0ull}
                                   : mount_->getObjectStore()->getBlobSize(
                                         treeEntry->getObjectId(), context);
              bool isSymlink = !symlinksEnabled_ || isDir
                  ? false
                  : treeEntry->getDtype() == dtype_t::Symlink;

              auto symlinkAttrsFut = isSymlink
                  ? mount_->getObjectStore()
                        ->getBlob(treeEntry->getObjectId(), context)
                        .thenValue(
                            [this,
                             path = path.copy(),
                             context = context.copy()](
                                std::shared_ptr<const Blob> blob)
                                -> ImmediateFuture<std::pair<
                                    std::optional<std::string>,
                                    bool>> {
                              auto content = blob->asString();
                              // ProjectedFS does consider / as a valid
                              // separator, but trying to open symlinks with
                              // forward slashes on Windows generally doesn't
                              // work. This also applies to having symlinks in
                              // places other than EdenFS. So we replace them
                              // with backslashes.
                              //
                              // Additionally, since creating a commit
                              // normalizes backward slashes to forward slashes
                              // in the commit itself, we need to normalize to
                              // turn them back into backtward ones. We need to
                              // do this here due to the fact that this also
                              // applies to absolute paths which most of the
                              // time contain UNC prefixes. For instance, if we
                              // created a symlink to the directory "C:\foo" and
                              // then tried to create a commit containing this
                              // symlink, we would end up with "//?/C:/foo" in
                              // the commit itself, and when checking out this
                              // commit, we would need to convert it to
                              // "\\?\C:\foo" so that we can properly check that
                              // this symlinks is a directory in
                              // `isFinalSymlinkPathDirectory`
                              std::replace(
                                  content.begin(), content.end(), '/', '\\');
                              return isFinalSymlinkPathDirectory(
                                         path, content, context)
                                  .thenValue(
                                      [content = std::move(content)](bool isDir)
                                          -> std::pair<
                                              std::optional<std::string>,
                                              bool> {
                                        return {content, isDir};
                                      });
                            })
                  : ImmediateFuture<
                        std::pair<std::optional<std::string>, bool>>{
                        {std::nullopt, false}};

              return collectAllSafe(pathFut, sizeFut, symlinkAttrsFut)
                  .thenValue(
                      [this, isDir, context = context.copy()](
                          std::tuple<
                              RelativePath,
                              uint64_t,
                              std::pair<std::optional<std::string>, bool>>
                              res) {
                        auto [path, size, symlinkAttrs] = std::move(res);
                        auto symlinkDestination = symlinkAttrs.first;
                        auto symlinkIsDirectory = symlinkAttrs.second;
                        auto lookupResult = LookupResult{
                            path,
                            size,
                            isDir || symlinkIsDirectory,
                            std::move(symlinkDestination)};

                        // We need to run the following asynchronously to
                        // avoid the risk of deadlocks when EdenFS recursively
                        // triggers this lookup call. In rare situation, this
                        // might happen during a checkout operation which is
                        // already holding locks that the code below also
                        // need.
                        folly::via(
                            getNotificationExecutor(),
                            [&mount = *mount_,
                             path = std::move(path),
                             context = context.copy()]() {
                              // Finally, let's tell the TreeInode that this
                              // file needs invalidation during update. This
                              // is run in a separate executor to avoid
                              // deadlocks. This is guaranteed to 1) run
                              // before any other changes to this inode, and
                              // 2) before checkout starts invalidating
                              // files/directories. This also cannot race with
                              // a decFsRefcount from
                              // TreeInode::invalidateChannelEntryCache due to
                              // getInodeSlow needing to acquire the content
                              // lock that invalidateChannelEntryCache is
                              // already holding.
                              mount.getInodeSlow(path, context)
                                  .thenValue([](InodePtr inode) {
                                    inode->incFsRefcount();
                                  })
                                  .get();
                            });

                        return std::optional{std::move(lookupResult)};
                      });
            })
            .thenTry(
                [this, path = std::move(path)](
                    folly::Try<std::optional<LookupResult>> result)
                    -> folly::Try<std::optional<LookupResult>> {
                  if (auto* exc =
                          result.tryGetExceptionObject<std::system_error>()) {
                    if (isEnoent(*exc)) {
                      if (path == kDotEdenConfigPath) {
                        return folly::Try{std::optional{LookupResult{
                            std::move(path),
                            dotEdenConfig_.length(),
                            false,
                            std::nullopt}}};
                      } else if (path == kDotEdenRelativePath) {
                        return folly::Try{std::optional{LookupResult{
                            std::move(path), 0, true, std::nullopt}}};
                      } else {
                        XLOGF(DBG6, "{}: File not found", path);
                        return folly::Try<std::optional<LookupResult>>{
                            std::nullopt};
                      }
                    }
                  }
                  return result;
                });
      });
}

ImmediateFuture<bool> PrjfsDispatcherImpl::access(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return mount_->getServerState()
      ->getFaultInjector()
      .checkAsync("PrjfsDispatcherImpl::access", path.view())
      .thenValue([this, path = std::move(path), context = context.copy()](
                     auto&&) mutable {
        return mount_->getTreeOrTreeEntry(path, context)
            .thenValue([](auto&&) { return true; })
            .thenTry([path = std::move(path)](folly::Try<bool> result) {
              if (auto* exc =
                      result.tryGetExceptionObject<std::system_error>()) {
                if (isEnoent(*exc)) {
                  if (path == kDotEdenRelativePath ||
                      path == kDotEdenConfigPath) {
                    return folly::Try<bool>{true};
                  } else {
                    return folly::Try<bool>{false};
                  }
                }
              }
              return result;
            });
      });
}

ImmediateFuture<std::string> PrjfsDispatcherImpl::read(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return mount_->getServerState()
      ->getFaultInjector()
      .checkAsync("PrjfsDispatcherImpl::read", path.view())
      .thenValue([this, path = std::move(path), context = context.copy()](
                     auto&&) mutable {
        return mount_->getTreeOrTreeEntry(path, context)
            .thenValue([context = context.copy(),
                        objectStore = mount_->getObjectStore()](
                           std::variant<std::shared_ptr<const Tree>, TreeEntry>
                               treeOrTreeEntry) {
              auto& treeEntry = std::get<TreeEntry>(treeOrTreeEntry);
              return objectStore->getBlob(treeEntry.getObjectId(), context)
                  .thenValue([](std::shared_ptr<const Blob> blob) {
                    // TODO(xavierd): directly return the Blob to the
                    // caller.
                    std::string res;
                    blob->getContents().appendTo(res);
                    return res;
                  });
            })
            .thenTry([this,
                      path = std::move(path)](folly::Try<std::string> result) {
              if (auto* exc =
                      result.tryGetExceptionObject<std::system_error>()) {
                if (isEnoent(*exc) && path == kDotEdenConfigPath) {
                  return folly::Try<std::string>{std::string(dotEdenConfig_)};
                }
              }
              return result;
            });
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
          XLOGF_EVERY_MS(
              DBG2,
              1000,
              "Out of order directory creation notification for: {}",
              path);

          /*
           * ProjectedFS notifications are asynchronous and sent after the
           * fact. This means that we can get a notification on a
           * file/directory before the parent directory notification has
           * been completed. This should be a very rare event and thus the
           * code below is pessimistic and will try to create all parent
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

enum class OnDiskStateTypes {
  MaterializedFile,
  MaterializedSymlink,
  MaterializedDirectory,
  NotPresent,
};

struct OnDiskState {
  OnDiskStateTypes type;
  std::optional<boost::filesystem::path> symlinkTarget;

  explicit OnDiskState(
      OnDiskStateTypes _type,
      std::optional<boost::filesystem::path> _target = std::nullopt)
      : type(_type), symlinkTarget(_target) {}
};

ImmediateFuture<OnDiskState> recheckDiskState(
    const EdenMount& mount,
    RelativePathPiece path,
    std::chrono::steady_clock::time_point receivedAt,
    int retry,
    OnDiskStateTypes expectedType);

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
    return recheckDiskState(
        mount, path, receivedAt, retry, OnDiskStateTypes::MaterializedFile);
  } else if (fileType == boost::filesystem::symlink_file) {
    if (mount.getCheckoutConfig()->getEnableWindowsSymlinks()) {
      auto symlinkTarget = boost::filesystem::read_symlink(boostPath, ec);
      if (ec.value() == 0) {
        return OnDiskState(
            OnDiskStateTypes::MaterializedSymlink, symlinkTarget);
      }
      return getOnDiskState(mount, path, receivedAt, retry + 1);
    }
    return OnDiskState(OnDiskStateTypes::MaterializedFile);
  } else if (fileType == boost::filesystem::reparse_file) {
    // Boost reports anything that is a reparse point which is not a symlink a
    // reparse_file. In particular, socket are reported as such.
    return OnDiskState(OnDiskStateTypes::MaterializedFile);
  } else if (fileType == boost::filesystem::directory_file) {
    return recheckDiskState(
        mount,
        path,
        receivedAt,
        retry,
        OnDiskStateTypes::MaterializedDirectory);
  } else if (fileType == boost::filesystem::file_not_found) {
    return OnDiskState(OnDiskStateTypes::NotPresent);
  } else if (fileType == boost::filesystem::status_error) {
    if (retry == 5) {
      XLOGF(WARN, "Assuming path is not present: {}", path);
      return OnDiskState(OnDiskStateTypes::NotPresent);
    }
    XLOGF(WARN, "Error: {} for path: {}", ec.message(), path);
    return ImmediateFuture{folly::futures::sleep(retry * 5ms)}.thenValue(
        [&mount, path = path.copy(), receivedAt, retry](folly::Unit&&) {
          return getOnDiskState(mount, path, receivedAt, retry + 1);
        });
  } else {
    return makeImmediateFuture<OnDiskState>(std::logic_error(
        fmt::format("Unknown file type {} for file {}", fileType, path)));
  }
}

ImmediateFuture<OnDiskState> recheckDiskState(
    const EdenMount& mount,
    RelativePathPiece path,
    std::chrono::steady_clock::time_point receivedAt,
    int retry,
    OnDiskStateTypes expectedType) {
  const auto elapsed = std::chrono::steady_clock::now() - receivedAt;
  const auto delay =
      mount.getEdenConfig()->prjfsDirectoryCreationDelay.getValue();
  if (elapsed < delay) {
    // See comment on EdenConfig::prjfsDirectoryCreationDelay for what's
    // going on here.
    auto timeToSleep =
        std::chrono::duration_cast<folly::HighResDuration>(delay - elapsed);
    return ImmediateFuture{folly::futures::sleep(timeToSleep)}.thenValue(
        [&mount, path = path.copy(), retry, receivedAt](folly::Unit&&) {
          return getOnDiskState(mount, path, receivedAt, retry);
        });
  }
  return OnDiskState(expectedType);
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
                if (state.type == OnDiskStateTypes::MaterializedDirectory) {
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
    OnDiskStateTypes diskStateType,
    std::optional<boost::filesystem::path> symlinkTarget,
    std::chrono::steady_clock::time_point receivedAt,
    const ObjectFetchContextPtr& context) {
  return createDirInode(mount, path.dirname().copy(), context)
      .thenValue([&mount,
                  path = std::move(path),
                  diskStateType,
                  symlinkTarget = std::move(symlinkTarget),
                  receivedAt,
                  context =
                      context.copy()](const TreeInodePtr treeInode) mutable {
        auto basename = path.basename();
        return treeInode->getOrLoadChild(basename, context)
            .thenTry(
                [&mount,
                 path = std::move(path),
                 treeInode,
                 diskStateType,
                 symlinkTarget = std::move(symlinkTarget),
                 receivedAt,
                 context = context.copy()](folly::Try<InodePtr> try_) mutable
                -> ImmediateFuture<folly::Unit> {
                  auto basename = path.basename();
                  if (try_.hasException()) {
                    if (auto* exc =
                            try_.tryGetExceptionObject<std::system_error>()) {
                      if (isEnoent(*exc)) {
                        if (diskStateType ==
                            OnDiskStateTypes::MaterializedDirectory) {
                          auto child = treeInode->mkdir(
                              basename, _S_IFDIR, InvalidationRequired::No);
                          child->incFsRefcount();
                          return recursivelyUpdateChildrens(
                              mount,
                              std::move(child),
                              std::move(path),
                              receivedAt,
                              context);
                        } else if (
                            diskStateType ==
                            OnDiskStateTypes::MaterializedSymlink) {
                          auto child = treeInode->symlink(
                              basename,
                              symlinkTarget.value().string(),
                              InvalidationRequired::No);
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
                  switch (diskStateType) {
                    case OnDiskStateTypes::MaterializedDirectory: {
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
                    case OnDiskStateTypes::NotPresent:
                    case OnDiskStateTypes::MaterializedFile:
                    case OnDiskStateTypes::MaterializedSymlink: {
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
                              [diskStateType,
                               symlinkTarget,
                               basename = basename.copy(),
                               treeInode](folly::Try<folly::Unit> try_)
                                  -> ImmediateFuture<folly::Unit> {
                                if (auto* exc = try_.tryGetExceptionObject<
                                                std::system_error>()) {
                                  if (!isEnoent(*exc)) {
                                    return makeImmediateFuture<folly::Unit>(
                                        try_.exception());
                                  }
                                }
                                if (diskStateType ==
                                    OnDiskStateTypes::MaterializedSymlink) {
                                  auto child = treeInode->symlink(
                                      basename,
                                      symlinkTarget.value().string(),
                                      InvalidationRequired::No);
                                  child->incFsRefcount();
                                } else {
                                  auto child = treeInode->mknod(
                                      basename,
                                      _S_IFREG,
                                      0,
                                      InvalidationRequired::No);
                                  child->incFsRefcount();
                                }
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
        switch (state.type) {
          case OnDiskStateTypes::MaterializedDirectory:
          case OnDiskStateTypes::MaterializedFile:
            return handleMaterializedFileNotification(
                mount,
                std::move(path),
                state.type,
                std::nullopt,
                receivedAt,
                context);
          case OnDiskStateTypes::MaterializedSymlink:
            return handleMaterializedFileNotification(
                mount,
                std::move(path),
                OnDiskStateTypes::MaterializedSymlink,
                std::move(state.symlinkTarget),
                receivedAt,
                context);
          case OnDiskStateTypes::NotPresent:
            return handleNotPresentFileNotification(
                mount, std::move(path), receivedAt, context);
        }
      });
}

/**
 * Matches EdenFS's view of a file/directory to it's state on disk. This is
 * mostly used in response to notifications about file modifications from PrjFS.
 * But can also be used to correct EdenFS's view of a file.
 *
 * Most callers are not prepared to handle an error so they will use the default
 * value for dfatal_error. When dfatal_error is true, the returned future never
 * contains an error, but eden may crash if an exception occurs. When
 * dfatal_error is false, the returned future may contain an exception
 * which occurred while trying to sync Eden to the filesystem.
 *
 */
ImmediateFuture<folly::Unit> fileNotification(
    EdenMount& mount,
    RelativePath path,
    const ObjectFetchContextPtr& context,
    bool dfatal_error = true) {
  auto receivedAt = std::chrono::steady_clock::now();
  folly::stop_watch<std::chrono::milliseconds> watch;

  // We need to make sure all the handling of the notification is done
  // non-immediately in the executor chosen by the caller thus creating a
  // not-ready ImmediateFuture to this effect.
  return makeNotReadyImmediateFuture()
      .thenValue([&mount, path, receivedAt, context = context.copy(), watch](
                     auto&&) mutable {
        auto fault = mount.getServerState()->getFaultInjector().checkAsync(
            "PrjfsDispatcherImpl::fileNotification", path);

        std::move(fault)
            .thenValue([&mount,
                        path = std::move(path),
                        receivedAt,
                        context = context.copy()](auto&&) {
              return fileNotificationImpl(
                  mount, std::move(path), receivedAt, context);
            })
            // Manually waiting for the future to make sure that a single
            // notification is handled with no interleaving with other
            // notifications.
            .get();
        mount.getStats()->addDuration(
            &PrjfsStats::queuedFileNotification, watch.elapsed());
        return folly::unit;
      })
      .thenError(
          [path, &mount, dfatal_error](const folly::exception_wrapper& ew) {
            if (ew.get_exception<QuietFault>()) {
              XLOGF(ERR, "While handling notification on: {}: {}", path, ew);
              return folly::unit;
            }

            // These should in theory never happen, but they sometimes happen
            // due to filesystem errors, antivirus scanning, etc. During
            // test, these should be treated as fatal errors, so we don't let
            // errors silently pass tests. In release builds, let's be less
            // aggressive and just log.
            mount.getServerState()->getStructuredLogger()->logEvent(
                PrjFSFileNotificationFailure{
                    folly::exceptionStr(ew).toStdString(), path.asString()});
            if (dfatal_error) {
              XLOGF(DFATAL, "While handling notification on: {}: {}", path, ew);
              return folly::unit;
            } else {
              XLOGF(ERR, "While handling notification on: {}: {}", path, ew);
              ew.throw_exception();
            }
          });
}

} // namespace

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileCreated(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(*mount_, std::move(path), context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::dirCreated(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(*mount_, std::move(path), context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileModified(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(*mount_, std::move(path), context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::fileRenamed(
    RelativePath oldPath,
    RelativePath newPath,
    const ObjectFetchContextPtr& context) {
  // A rename is just handled like 2 notifications separate notifications on
  // the old and new paths.
  auto oldNotification = fileNotification(*mount_, std::move(oldPath), context);
  auto newNotification = fileNotification(*mount_, std::move(newPath), context);

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
  return fileNotification(*mount_, std::move(path), context);
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
  return fileNotification(*mount_, std::move(path), context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preDirDelete(
    RelativePath path,
    const ObjectFetchContextPtr& /*context*/) {
  if (path == kDotEdenRelativePath) {
    return makeImmediateFuture<folly::Unit>(
        std::system_error(EPERM, std::generic_category()));
  }

  auto redirection_targets =
      mount_->getCheckoutConfig()->getLatestRedirectionTargets();
  if (redirection_targets->find(path.asString()) !=
      redirection_targets->end()) {
    auto full_path = mount_->getPath() + path;
    // If it is a symlink then clear its content, else if directory/file then
    // let post delete notification handle it.
    if (std::filesystem::is_symlink(full_path.c_str())) {
      XLOGF(
          INFO,
          "Redirected path '{}' is directed to be deleted. Not actually deleting the directory, instead deleting its content.",
          full_path.asString());
      handleRedirectedPathPreDeletion(
          full_path, redirection_targets->at(path.asString()));

      // Returning error will error out delete operation leading to not
      // executing actual delete operation and will not trigger post delete
      // notification.
      return makeImmediateFuture<folly::Unit>(
          std::system_error(EPERM, std::generic_category()));
    }
  }
  return folly::unit;
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::preFileConvertedToFull(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  // this is an asynchronous notification, so we have to treat this just like
  // all the other write notifications.
  return fileNotification(*mount_, std::move(path), context);
}

ImmediateFuture<folly::Unit> PrjfsDispatcherImpl::matchEdenViewOfFileToFS(
    RelativePath path,
    const ObjectFetchContextPtr& context) {
  return fileNotification(
      *mount_, std::move(path), context, /*dfatal_error=*/false);
}

/**
 * This function is called when a redirected path is being deleted. Instead of
 * deleting the path, it will delete the content of symlink's target.
 */
int PrjfsDispatcherImpl::handleRedirectedPathPreDeletion(
    AbsolutePathPiece symlinkPath,
    std::string targetPath) {
  auto symlinkTarget = std::filesystem::read_symlink(symlinkPath.asString());
  if (symlinkTarget != targetPath) {
    XLOGF(
        ERR,
        "Symlink target '{}' is not same as config target '{}'. Overriding with the config target.",
        symlinkPath,
        targetPath);
    // Remove the symlink and create a new symlink to the config target.
    try {
      std::filesystem::remove(symlinkPath.asString());
      XLOG(DBG2, "Symlink deletion successful.");
    } catch (const std::filesystem::filesystem_error& e) {
      XLOGF(INFO, "Error deleting symlink: {}", e.what());
      return 1;
    }
  }

  if (std::filesystem::exists(targetPath.c_str())) {
    XLOGF(
        INFO,
        "Symlink target '{}' already exists. Trying to delete its content.",
        targetPath.c_str());
    try {
      std::filesystem::remove_all(targetPath.c_str());
      std::filesystem::create_directory(targetPath.c_str());
      XLOG(DBG2, "Symlink target contents deleted successfully.");
    } catch (const std::filesystem::filesystem_error& e) {
      XLOGF(INFO, "Error deleting symlink target contents: {}", e.what());
      return 1;
    }
  } else {
    // Create a symlink to the config target.
    std::filesystem::create_symlink(targetPath.c_str(), symlinkPath.asString());
  }

  return 0;
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
  folly::stop_watch<std::chrono::microseconds> timer{};
  return ImmediateFuture{folly::via(
                             getNotificationExecutor(),
                             [this, timer = std::move(timer)]() {
                               this->mount_->getStats()->addDuration(
                                   &PrjfsStats::filesystemSync,
                                   timer.elapsed());
                               this->mount_->getStats()->increment(
                                   &PrjfsStats::filesystemSyncSuccessful);
                               return folly::unit;
                             })
                             .semi()}
      .thenError(
          [this](const folly::exception_wrapper& ew)
              -> ImmediateFuture<folly::Unit> {
            this->mount_->getStats()->increment(
                &PrjfsStats::filesystemSyncFailure);
            ew.throw_exception();
          });
}

} // namespace facebook::eden

#endif
