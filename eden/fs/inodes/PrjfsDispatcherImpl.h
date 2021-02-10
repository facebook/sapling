/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/prjfs/PrjfsDispatcher.h"

namespace facebook::eden {

class EdenMount;

class PrjfsDispatcherImpl : public PrjfsDispatcher {
 public:
  explicit PrjfsDispatcherImpl(EdenMount* mount);

  folly::Future<std::vector<FileMetadata>> opendir(
      RelativePathPiece path,
      ObjectFetchContext& context) override;

  folly::Future<std::optional<LookupResult>> lookup(
      RelativePath path,
      ObjectFetchContext& context) override;

  folly::Future<bool> access(RelativePath path, ObjectFetchContext& context)
      override;

  folly::Future<std::string> read(
      RelativePath path,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> newFileCreated(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileOverwritten(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileHandleClosedFileModified(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> preRename(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> fileHandleClosedFileDeleted(
      RelativePath relPath,
      RelativePath destPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

  folly::Future<folly::Unit> preSetHardlink(
      RelativePath oldPath,
      RelativePath newPath,
      bool isDirectory,
      ObjectFetchContext& context) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;

  const std::string dotEdenConfig_;
};

} // namespace facebook::eden
