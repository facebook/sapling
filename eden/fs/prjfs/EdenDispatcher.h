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
#include "eden/fs/prjfs/Dispatcher.h"
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/utils/Guid.h"

namespace facebook {
namespace eden {

class EdenMount;
class PrjfsRequestContext;

class EdenDispatcher : public Dispatcher {
 public:
  explicit EdenDispatcher(EdenMount* mount);

  folly::Future<folly::Unit> opendir(
      RelativePathPiece path,
      const Guid guid,
      ObjectFetchContext& context) override;

  void closedir(const Guid& guid);

  HRESULT getEnumerationData(
      const PRJ_CALLBACK_DATA& callbackData,
      const GUID& enumerationId,
      PCWSTR searchExpression,
      PRJ_DIR_ENTRY_BUFFER_HANDLE dirEntryBufferHandle) noexcept override;

  folly::Future<std::optional<InodeMetadata>> lookup(
      RelativePath path,
      ObjectFetchContext& context) override;

  folly::Future<bool> access(RelativePath path, ObjectFetchContext& context)
      override;

  /** Returns the entire content of the file at path.
   *
   * In the future, this will return only what's in between offset and
   * offset+length.
   */
  folly::Future<std::string> read(
      RelativePath path,
      uint64_t offset,
      uint32_t length,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> newFileCreated(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileOverwritten(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileHandleClosedFileModified(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileRenamed(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> preRename(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileHandleClosedFileDeleted(
      RelativePathPiece relPath,
      RelativePathPiece destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> preSetHardlink(
      RelativePathPiece oldPath,
      RelativePathPiece newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

 private:
  // The EdenMount that owns this EdenDispatcher.
  EdenMount* const mount_;

  // Set of currently active directory enumerations.
  folly::Synchronized<folly::F14FastMap<Guid, Enumerator>> enumSessions_;

  const std::string dotEdenConfig_;
};

} // namespace eden
} // namespace facebook
