/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace facebook::eden {

class EdenMount;

class PrjfsDispatcherImpl : public PrjfsDispatcher {
 public:
  explicit PrjfsDispatcherImpl(EdenMount* mount);

  EdenTimestamp getLastCheckoutTime() const override;

  ImmediateFuture<std::vector<PrjfsDirEntry>> opendir(
      RelativePath path,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<std::optional<LookupResult>> lookup(
      RelativePath path,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<bool> access(
      RelativePath path,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<std::string> read(
      RelativePath path,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> fileCreated(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> dirCreated(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> fileModified(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> preDirRename(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> preFileRename(
      RelativePath oldPath,
      RelativePath newPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> fileDeleted(
      RelativePath oldPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> preFileDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> dirDeleted(
      RelativePath oldPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> preDirDelete(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> preFileConvertedToFull(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> matchEdenViewOfFileToFS(
      RelativePath relPath,
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<folly::Unit> waitForPendingNotifications() override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;

  const std::string dotEdenConfig_;

  bool symlinksEnabled_;
};

} // namespace facebook::eden
