/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "folly/portability/Windows.h"

#include <ProjectedFSLib.h>
#include <folly/Synchronized.h>
#include <folly/container/F14Map.h>
#include <cstdint>
#include <cstring>
#include <string>
#include "eden/fs/win/mount/Enumerator.h"
#include "eden/fs/win/utils/Guid.h"

namespace facebook {
namespace eden {

class EdenMount;
class PrjfsRequestContext;

struct InodeMetadata {
  // To ensure that the OS has a record of the canonical file name, and not
  // just whatever case was used to lookup the file, we capture the
  // relative path here.
  RelativePath path;
  size_t size;
  bool isDir;
};

class EdenDispatcher {
 public:
  explicit EdenDispatcher(EdenMount* mount);

  EdenStats* getStats() {
    return mount_->getStats();
  }

  folly::Future<folly::Unit>
  opendir(RelativePathPiece path, const Guid guid, ObjectFetchContext& context);

  void closedir(const Guid& guid);

  HRESULT getEnumerationData(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept;

  folly::Future<std::optional<InodeMetadata>> lookup(
      RelativePath path,
      ObjectFetchContext& context);

  folly::Future<bool> access(RelativePath path, ObjectFetchContext& context);

  HRESULT
  getFileData(
      const PRJ_CALLBACK_DATA& callbackData,
      uint64_t byteOffset,
      uint32_t length) noexcept;

  folly::Future<folly::Unit> newFileCreated(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> fileOverwritten(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> fileHandleClosedFileModified(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> fileRenamed(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> preRename(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> fileHandleClosedFileDeleted(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context);

  folly::Future<folly::Unit> preSetHardlink(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context);

 private:
  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;

  // Set of currently active directory enumerations.
  folly::Synchronized<folly::F14FastMap<Guid, Enumerator>> enumSessions_;

  const std::string dotEdenConfig_;
};

} // namespace eden
} // namespace facebook
