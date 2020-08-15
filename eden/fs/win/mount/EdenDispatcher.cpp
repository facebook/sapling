/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <cpptoml.h>
#include <folly/Format.h>
#include <folly/logging/xlog.h>
#include "ProjectedFSLib.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/utils/Guid.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

using folly::sformat;
using std::make_unique;
using std::wstring;

namespace facebook {
namespace eden {

namespace {
struct PrjAlignedBufferDeleter {
  void operator()(void* buffer) noexcept {
    ::PrjFreeAlignedBuffer(buffer);
  }
};

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

constexpr uint32_t kMinChunkSize = 512 * 1024; // 512 KiB
constexpr uint32_t kMaxChunkSize = 5 * 1024 * 1024; // 5 MiB

EdenDispatcher::EdenDispatcher(EdenMount* mount)
    : mount_{mount}, dotEdenConfig_{makeDotEdenConfig(*mount)} {}

HRESULT EdenDispatcher::startEnumeration(
    const PRJ_CALLBACK_DATA& callbackData,
    const GUID& enumerationId) noexcept {
  try {
    auto relPath = RelativePath(callbackData.FilePathName);
    auto guid = Guid(enumerationId);

    FB_LOGF(
        mount_->getStraceLogger(), DBG7, "opendir({}, guid={})", relPath, guid);

    auto list = mount_->getInode(relPath)
                    .thenValue([](const InodePtr inode) {
                      auto treePtr = inode.asTreePtr();
                      return treePtr->readdir();
                    })
                    .get();

    auto [iterator, inserted] =
        enumSessions_.wlock()->emplace(guid, std::move(list));
    DCHECK(inserted);
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT EdenDispatcher::endEnumeration(const GUID& enumerationId) noexcept {
  try {
    auto guid = Guid(enumerationId);
    FB_LOGF(mount_->getStraceLogger(), DBG7, "releasedir({})", guid);

    auto erasedCount = enumSessions_.wlock()->erase(guid);
    DCHECK(erasedCount == 1);
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
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

HRESULT
EdenDispatcher::getFileInfo(const PRJ_CALLBACK_DATA& callbackData) noexcept {
  try {
    auto relPath = RelativePath(callbackData.FilePathName);
    FB_LOGF(mount_->getStraceLogger(), DBG7, "lookup({})", relPath);

    struct InodeMetadata {
      // To ensure that the OS has a record of the canonical file name, and not
      // just whatever case was used to lookup the file, we capture the
      // relative path here.
      RelativePath path;
      size_t size;
      bool isDir;
    };

    return mount_->getInode(relPath)
        .thenValue(
            [](const InodePtr inode)
                -> folly::Future<std::optional<InodeMetadata>> {
              return inode->stat(ObjectFetchContext::getNullContext())
                  .thenValue([inode = std::move(inode)](struct stat&& stat) {
                    // Ensure that the OS has a record of the canonical
                    // file name, and not just whatever case was used to
                    // lookup the file
                    size_t size = stat.st_size;
                    return InodeMetadata{
                        *inode->getPath(), size, inode->isDir()};
                  });
            })
        .thenError(
            folly::tag_t<std::system_error>{},
            [relPath = std::move(relPath), this](const std::system_error& ex)
                -> folly::Future<std::optional<InodeMetadata>> {
              if (isEnoent(ex)) {
                if (relPath == kDotEdenConfigPath) {
                  return folly::makeFuture(
                      InodeMetadata{relPath, dotEdenConfig_.length(), false});
                } else {
                  XLOG(DBG6) << relPath << ": File not found";
                  return folly::makeFuture(std::nullopt);
                }
              }
              return folly::makeFuture<std::optional<InodeMetadata>>(ex);
            })
        .thenValue([context = callbackData.NamespaceVirtualizationContext](
                       const std::optional<InodeMetadata>&& metadata) {
          if (!metadata) {
            return HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND);
          }

          PRJ_PLACEHOLDER_INFO placeholderInfo{};
          placeholderInfo.FileBasicInfo.IsDirectory = metadata->isDir;
          placeholderInfo.FileBasicInfo.FileSize = metadata->size;
          auto inodeName = metadata->path.wide();

          HRESULT result = PrjWritePlaceholderInfo(
              context,
              inodeName.c_str(),
              &placeholderInfo,
              sizeof(placeholderInfo));

          if (FAILED(result)) {
            XLOGF(
                DBG6,
                "{}: {:x} ({})",
                metadata->path,
                result,
                win32ErrorToString(result));
          }

          return result;
        })
        .thenError(
            folly::tag_t<std::exception>{},
            [](const std::exception& ex) { return exceptionToHResult(ex); })
        .get();
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

HRESULT
EdenDispatcher::queryFileName(const PRJ_CALLBACK_DATA& callbackData) noexcept {
  try {
    auto relPath = RelativePath(callbackData.FilePathName);
    FB_LOGF(mount_->getStraceLogger(), DBG7, "access({})", relPath);

    return mount_->getInode(relPath)
        .thenValue([](const InodePtr) { return S_OK; })
        .thenError(
            folly::tag_t<std::system_error>{},
            [relPath = std::move(relPath)](const std::system_error& ex) {
              if (isEnoent(ex)) {
                if (relPath == kDotEdenConfigPath) {
                  return folly::makeFuture(S_OK);
                }
                return folly::makeFuture(
                    HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND));
              }
              return folly::makeFuture<HRESULT>(ex);
            })
        .get();
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

namespace {

HRESULT readMultipleFileChunks(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const GUID& dataStreamId,
    const std::string& content,
    uint64_t startOffset,
    uint64_t length,
    uint64_t chunkSize) {
  HRESULT result;
  std::unique_ptr<void, PrjAlignedBufferDeleter> writeBuffer{
      PrjAllocateAlignedBuffer(namespaceVirtualizationContext, chunkSize)};

  if (writeBuffer.get() == nullptr) {
    return E_OUTOFMEMORY;
  }

  uint64_t remainingLength = length;

  while (remainingLength > 0) {
    uint64_t copySize = std::min(remainingLength, chunkSize);

    //
    // TODO(puneetk): Once backing store has the support for chunking the file
    // contents, we can read the chunks of large files here and then write
    // them to FS.
    //
    // TODO(puneetk): Build an interface to backing store so that we can pass
    // the aligned buffer to avoid coping here.
    //
    RtlCopyMemory(writeBuffer.get(), content.data() + startOffset, copySize);

    // Write the data to the file in the local file system.
    result = PrjWriteFileData(
        namespaceVirtualizationContext,
        &dataStreamId,
        writeBuffer.get(),
        startOffset,
        folly::to_narrow(copySize));

    if (FAILED(result)) {
      return result;
    }

    remainingLength -= copySize;
    startOffset += copySize;
  }

  return S_OK;
}

HRESULT readSingleFileChunk(
    PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT namespaceVirtualizationContext,
    const GUID& dataStreamId,
    const std::string& content,
    uint64_t startOffset,
    uint64_t length) {
  return readMultipleFileChunks(
      namespaceVirtualizationContext,
      dataStreamId,
      content,
      /*startOffset=*/startOffset,
      /*length=*/length,
      /*writeLength=*/length);
}

} // namespace

static uint64_t BlockAlignTruncate(uint64_t ptr, uint32_t alignment) {
  return ((ptr) & (0 - (static_cast<uint64_t>(alignment))));
}

HRESULT
EdenDispatcher::getFileData(
    const PRJ_CALLBACK_DATA& callbackData,
    uint64_t byteOffset,
    uint32_t length) noexcept {
  try {
    auto relPath = RelativePath(callbackData.FilePathName);
    FB_LOGF(
        mount_->getStraceLogger(),
        DBG7,
        "read({}, off={}, len={})",
        relPath,
        byteOffset,
        length);

    auto content =
        mount_->getInode(relPath)
            .thenValue([](const InodePtr inode) {
              auto fileInode = inode.asFilePtr();
              return fileInode->readAll(ObjectFetchContext::getNullContext());
            })
            .thenError(
                folly::tag_t<std::system_error>{},
                [relPath = std::move(relPath),
                 this](const std::system_error& ex) {
                  if (isEnoent(ex) && relPath == kDotEdenConfigPath) {
                    return folly::makeFuture<std::string>(
                        std::string(dotEdenConfig_));
                  }
                  return folly::makeFuture<std::string>(ex);
                })
            .get();

    //
    // We should return file data which is smaller than
    // our kMaxChunkSize and meets the memory alignment requirements
    // of the virtualization instance's storage device.
    //

    if (content.length() <= kMinChunkSize) {
      //
      // If the file is small - copy the whole file in one shot.
      //
      return readSingleFileChunk(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          content,
          /*startOffset=*/0,
          /*writeLength=*/content.length());

    } else if (length <= kMaxChunkSize) {
      //
      // If the request is with in our kMaxChunkSize - copy the entire request.
      //
      return readSingleFileChunk(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          content,
          /*startOffset=*/byteOffset,
          /*writeLength=*/length);
    } else {
      //
      // When the request is larger than kMaxChunkSize we split the
      // request into multiple chunks.
      //
      PRJ_VIRTUALIZATION_INSTANCE_INFO instanceInfo;
      HRESULT result = PrjGetVirtualizationInstanceInfo(
          callbackData.NamespaceVirtualizationContext, &instanceInfo);

      if (FAILED(result)) {
        return result;
      }

      uint64_t startOffset = byteOffset;
      uint64_t endOffset = BlockAlignTruncate(
          startOffset + kMaxChunkSize, instanceInfo.WriteAlignment);
      DCHECK(endOffset > 0);
      DCHECK(endOffset > startOffset);

      uint64_t chunkSize = endOffset - startOffset;
      return readMultipleFileChunks(
          callbackData.NamespaceVirtualizationContext,
          callbackData.DataStreamId,
          content,
          /*startOffset=*/startOffset,
          /*length=*/length,
          /*chunkSize=*/chunkSize);
    }
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
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

folly::Future<folly::Unit> newFileCreated(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto relPath = RelativePath(path);
  FB_LOGF(
      mount.getStraceLogger(),
      DBG7,
      "{}({})",
      isDirectory ? "mkdir" : "mknod",
      relPath);
  return createFile(mount, relPath, isDirectory);
}

folly::Future<folly::Unit> fileOverwritten(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto relPath = RelativePath(path);
  FB_LOGF(mount.getStraceLogger(), DBG7, "overwrite({})", relPath);
  return materializeFile(mount, relPath);
}

folly::Future<folly::Unit> fileHandleClosedFileModified(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto relPath = RelativePath(path);
  FB_LOGF(mount.getStraceLogger(), DBG7, "modified({})", relPath);
  return materializeFile(mount, relPath);
}

folly::Future<folly::Unit> fileRenamed(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto oldPath = RelativePath(path);
  auto newPath = RelativePath(destPath);

  FB_LOGF(mount.getStraceLogger(), DBG7, "rename({} -> {})", oldPath, newPath);

  // When files are moved in and out of the repo, the rename paths are
  // empty, handle these like creation/removal of files.
  if (oldPath.empty()) {
    return createFile(mount, newPath, isDirectory);
  } else if (newPath.empty()) {
    return removeFile(mount, oldPath, isDirectory);
  } else {
    return renameFile(mount, oldPath, newPath);
  }
}

folly::Future<folly::Unit> preRename(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  FB_LOGF(
      mount.getStraceLogger(),
      DBG7,
      "prerename({} -> {})",
      RelativePath(path),
      RelativePath(destPath));
  return folly::unit;
}

folly::Future<folly::Unit> fileHandleClosedFileDeleted(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto oldPath = RelativePath(path);
  FB_LOGF(
      mount.getStraceLogger(),
      DBG7,
      "{}({})",
      isDirectory ? "rmdir" : "unlink",
      oldPath);
  return removeFile(mount, oldPath, isDirectory);
}

folly::Future<folly::Unit> preSetHardlink(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory) {
  auto relPath = RelativePath(path);
  FB_LOGF(mount.getStraceLogger(), DBG7, "link({})", relPath);
  return folly::makeFuture<folly::Unit>(makeHResultErrorExplicit(
      HRESULT_FROM_WIN32(ERROR_ACCESS_DENIED),
      sformat("Hardlinks are not supported: {}", relPath)));
}

typedef folly::Future<folly::Unit> (*NotificationHandler)(
    const EdenMount& mount,
    PCWSTR path,
    PCWSTR destPath,
    bool isDirectory);

const std::unordered_map<PRJ_NOTIFICATION, NotificationHandler> handlerMap = {
    {PRJ_NOTIFICATION_NEW_FILE_CREATED, newFileCreated},
    {PRJ_NOTIFICATION_FILE_OVERWRITTEN, fileOverwritten},
    {PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_MODIFIED,
     fileHandleClosedFileModified},
    {PRJ_NOTIFICATION_FILE_RENAMED, fileRenamed},
    {PRJ_NOTIFICATION_PRE_RENAME, preRename},
    {PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_DELETED,
     fileHandleClosedFileDeleted},
    {PRJ_NOTIFICATION_PRE_SET_HARDLINK, preSetHardlink},
};

} // namespace

HRESULT EdenDispatcher::notification(
    const PRJ_CALLBACK_DATA& callbackData,
    bool isDirectory,
    PRJ_NOTIFICATION notificationType,
    PCWSTR destinationFileName,
    PRJ_NOTIFICATION_PARAMETERS& notificationParameters) noexcept {
  try {
    auto it = handlerMap.find(notificationType);
    if (it == handlerMap.end()) {
      XLOG(WARN) << "Unrecognized notification: " << notificationType;
      return HRESULT_FROM_WIN32(ERROR_INVALID_PARAMETER);
    } else {
      it->second(
            *mount_,
            callbackData.FilePathName,
            destinationFileName,
            isDirectory)
          .get();
    }
    return S_OK;
  } catch (const std::exception& ex) {
    return exceptionToHResult(ex);
  }
}

} // namespace eden
} // namespace facebook
