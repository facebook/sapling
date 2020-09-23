/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <cpptoml.h>
#include <fmt/format.h>
#include <folly/logging/xlog.h>
#include "ProjectedFSLib.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/prjfs/EdenDispatcher.h"
#include "eden/fs/prjfs/PrjfsRequestContext.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/Guid.h"
#include "eden/fs/utils/StringConv.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/WinError.h"

namespace facebook {
namespace eden {

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

EdenDispatcher::EdenDispatcher(EdenMount* mount)
    : mount_{mount}, dotEdenConfig_{makeDotEdenConfig(*mount)} {}

folly::Future<folly::Unit> EdenDispatcher::opendir(
    RelativePathPiece path,
    const Guid guid,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "opendir({}, guid={})", path, guid);

  return mount_->getInode(path)
      .thenValue([](const InodePtr inode) {
        auto treePtr = inode.asTreePtr();
        return treePtr->readdir();
      })
      .thenValue([this, guid = std::move(guid)](auto&& dirents) {
        auto [iterator, inserted] =
            enumSessions_.wlock()->emplace(guid, std::move(dirents));
        DCHECK(inserted);

        return folly::unit;
      });
}

void EdenDispatcher::closedir(const Guid& guid) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "closedir({})", guid);

  auto erasedCount = enumSessions_.wlock()->erase(guid);
  DCHECK(erasedCount == 1);
}

HRESULT EdenDispatcher::getEnumerationData(
    const PRJ_CALLBACK_DATA& callbackData,
    const GUID& enumerationId,
    PCWSTR searchExpression,
    PRJ_DIR_ENTRY_BUFFER_HANDLE bufferHandle) noexcept {
  try {
    auto guid = Guid(enumerationId);
    FB_LOGF(
        mount_->getStraceLogger(),
        DBG7,
        "readdir({}, searchExpression={})",
        guid,
        searchExpression == nullptr
            ? "<nullptr>"
            : wideToMultibyteString<std::string>(searchExpression));

    auto lockedSessions = enumSessions_.rlock();
    auto sessionIterator = lockedSessions->find(guid);
    if (sessionIterator == lockedSessions->end()) {
      XLOG(DBG5) << "Enum instance not found: "
                 << RelativePath(callbackData.FilePathName);
      return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
    }

    auto shouldRestart =
        bool(callbackData.Flags & PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN);

    // We won't ever get concurrent callbacks for a given enumeration, it is
    // therefore safe to modify the session here even though we do not hold an
    // exclusive lock to it.
    auto& session = const_cast<Enumerator&>(sessionIterator->second);

    if (session.isSearchExpressionEmpty() || shouldRestart) {
      if (searchExpression != nullptr) {
        session.saveExpression(searchExpression);
      } else {
        session.saveExpression(L"*");
      }
    }

    if (shouldRestart) {
      session.restart();
    }

    //
    // Traverse the list enumeration list and fill the remaining entry. Start
    // from where the last call left off.
    //
    for (const FileMetadata* entry; (entry = session.current());
         session.advance()) {
      auto fileInfo = PRJ_FILE_BASIC_INFO();

      fileInfo.IsDirectory = entry->isDirectory;
      fileInfo.FileSize = entry->size;

      XLOGF(
          DBG6,
          "Enum {} {} size= {}",
          PathComponent(entry->name),
          fileInfo.IsDirectory ? "Dir" : "File",
          fileInfo.FileSize);

      if (S_OK !=
          PrjFillDirEntryBuffer(entry->name.c_str(), &fileInfo, bufferHandle)) {
        // We are out of buffer space. This entry didn't make it. Return without
        // increment.
        return S_OK;
      }
    }
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

folly::Future<std::optional<InodeMetadata>> EdenDispatcher::lookup(
    RelativePath path,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "lookup({})", path);

  return mount_->getInode(path)
      .thenValue(
          [&context](const InodePtr inode) mutable
          -> folly::Future<std::optional<InodeMetadata>> {
            return inode->stat(context).thenValue(
                [inode = std::move(inode)](struct stat&& stat) {
                  // Ensure that the OS has a record of the canonical
                  // file name, and not just whatever case was used to
                  // lookup the file
                  size_t size = stat.st_size;
                  return InodeMetadata{*inode->getPath(), size, inode->isDir()};
                });
          })
      .thenError(
          folly::tag_t<std::system_error>{},
          [path = std::move(path), this](const std::system_error& ex)
              -> folly::Future<std::optional<InodeMetadata>> {
            if (isEnoent(ex)) {
              if (path == kDotEdenConfigPath) {
                return folly::makeFuture(InodeMetadata{
                    std::move(path), dotEdenConfig_.length(), false});
              } else {
                XLOG(DBG6) << path << ": File not found";
                return folly::makeFuture(std::nullopt);
              }
            }
            return folly::makeFuture<std::optional<InodeMetadata>>(ex);
          });
}

folly::Future<bool> EdenDispatcher::access(
    RelativePath path,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "access({})", path);

  return mount_->getInode(path)
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

folly::Future<std::string> EdenDispatcher::read(
    RelativePath path,
    uint64_t byteOffset,
    uint32_t length,
    ObjectFetchContext& context) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "read({}, off={}, len={})",
      path,
      byteOffset,
      length);

  return mount_->getInode(path)
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
    const RelativePathPiece path) {
  return mount.getInode(path)
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
                  treeInode->mkdir(
                      parent.basename(), _S_IFDIR, InvalidationRequired::No);
                } catch (std::system_error& ex) {
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
    bool isDirectory) {
  return createDirInode(mount, path.dirname())
      .thenValue([=, &mount](const TreeInodePtr treeInode) {
        if (isDirectory) {
          try {
            treeInode->mkdir(
                path.basename(), _S_IFDIR, InvalidationRequired::No);
          } catch (std::system_error& ex) {
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
          treeInode->mknod(
              path.basename(), _S_IFREG, 0, InvalidationRequired::No);
        }

        return folly::makeFuture(folly::unit);
      });
}

folly::Future<folly::Unit> materializeFile(
    const EdenMount& mount,
    const RelativePathPiece path) {
  return mount.getInode(path).thenValue([](const InodePtr inode) {
    auto fileInode = inode.asFilePtr();
    fileInode->materialize();
    return folly::unit;
  });
}

folly::Future<folly::Unit> renameFile(
    const EdenMount& mount,
    const RelativePathPiece oldPath,
    const RelativePathPiece newPath) {
  auto oldParentInode = createDirInode(mount, oldPath.dirname());
  auto newParentInode = createDirInode(mount, newPath.dirname());

  return std::move(oldParentInode)
      .thenValue([=, newParentInode = std::move(newParentInode)](
                     const TreeInodePtr oldParentTreePtr) mutable {
        return std::move(newParentInode)
            .thenValue([=, oldParentTreePtr = std::move(oldParentTreePtr)](
                           const TreeInodePtr newParentTreePtr) {
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
      });
}

folly::Future<folly::Unit> removeFile(
    const EdenMount& mount,
    const RelativePathPiece path,
    bool isDirectory) {
  return mount.getInode(path.dirname()).thenValue([=](const InodePtr inode) {
    auto treeInodePtr = inode.asTreePtr();
    if (isDirectory) {
      return treeInodePtr->rmdir(path.basename(), InvalidationRequired::No);
    } else {
      return treeInodePtr->unlink(path.basename(), InvalidationRequired::No);
    }
  });
}

} // namespace

folly::Future<folly::Unit> EdenDispatcher::newFileCreated(
    RelativePathPiece relPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "{}({})",
      isDirectory ? "mkdir" : "mknod",
      relPath);
  return createFile(*mount_, relPath, isDirectory);
}

folly::Future<folly::Unit> EdenDispatcher::fileOverwritten(
    RelativePathPiece relPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "overwrite({})", relPath);
  return materializeFile(*mount_, relPath);
}

folly::Future<folly::Unit> EdenDispatcher::fileHandleClosedFileModified(
    RelativePathPiece relPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "modified({})", relPath);
  return materializeFile(*mount_, relPath);
}

folly::Future<folly::Unit> EdenDispatcher::fileRenamed(
    RelativePathPiece oldPath,
    RelativePathPiece newPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(
      mount_->getStraceLogger(), DBG7, "rename({} -> {})", oldPath, newPath);

  // When files are moved in and out of the repo, the rename paths are
  // empty, handle these like creation/removal of files.
  if (oldPath.empty()) {
    return createFile(*mount_, newPath, isDirectory);
  } else if (newPath.empty()) {
    return removeFile(*mount_, oldPath, isDirectory);
  } else {
    return renameFile(*mount_, oldPath, newPath);
  }
}

folly::Future<folly::Unit> EdenDispatcher::preRename(
    RelativePathPiece oldPath,
    RelativePathPiece newPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(
      mount_->getStraceLogger(), DBG7, "prerename({} -> {})", oldPath, newPath);
  return folly::unit;
}

folly::Future<folly::Unit> EdenDispatcher::fileHandleClosedFileDeleted(
    RelativePathPiece oldPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(
      mount_->getStraceLogger(),
      DBG7,
      "{}({})",
      isDirectory ? "rmdir" : "unlink",
      oldPath);
  return removeFile(*mount_, oldPath, isDirectory);
}

folly::Future<folly::Unit> EdenDispatcher::preSetHardlink(
    RelativePathPiece relPath,
    RelativePathPiece destPath,
    bool isDirectory,
    ObjectFetchContext& context) {
  FB_LOGF(mount_->getStraceLogger(), DBG7, "link({})", relPath);
  return folly::makeFuture<folly::Unit>(makeHResultErrorExplicit(
      HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED),
      fmt::format(FMT_STRING("Hardlinks are not supported: {}"), relPath)));
}

} // namespace eden
} // namespace facebook
