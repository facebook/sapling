/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/executors/SequencedExecutor.h>
#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook::eden {

class EdenMount;

class PrjfsDispatcherImpl : public PrjfsDispatcher {
 public:
  explicit PrjfsDispatcherImpl(EdenMount* mount);

  ImmediateFuture<std::vector<PrjfsDirEntry>> opendir(
      RelativePath path,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<std::optional<LookupResult>> lookup(
      RelativePath path,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<bool> access(
      RelativePath path,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<std::string> read(
      RelativePath path,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> fileCreated(
      RelativePath relPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> dirCreated(
      RelativePath relPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> fileModified(
      RelativePath relPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> preDirRename(
      RelativePath oldPath,
      RelativePath newPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> preFileRename(
      RelativePath oldPath,
      RelativePath newPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> fileDeleted(
      RelativePath oldPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> preFileDelete(
      RelativePath relPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> dirDeleted(
      RelativePath oldPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> preDirDelete(
      RelativePath relPath,
      std::shared_ptr<ObjectFetchContext> context) override;

  ImmediateFuture<folly::Unit> waitForPendingNotifications() override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;

  UnboundedQueueExecutor executor_;
  // All the notifications are dispatched to this executor. The
  // waitForPendingNotifications implementation depends on this being a
  // SequencedExecutor.
  folly::Executor::KeepAlive<folly::SequencedExecutor> notificationExecutor_;

  const std::string dotEdenConfig_;
};

} // namespace facebook::eden
