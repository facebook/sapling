/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/executors/SequencedExecutor.h>
#include "eden/fs/prjfs/PrjfsDispatcher.h"

namespace facebook::eden {

class EdenMount;

class PrjfsDispatcherImpl : public PrjfsDispatcher {
 public:
  explicit PrjfsDispatcherImpl(EdenMount* mount);

  ImmediateFuture<std::vector<PrjfsDirEntry>> opendir(
      RelativePath path,
      ObjectFetchContext& context) override;

  ImmediateFuture<std::optional<LookupResult>> lookup(
      RelativePath path,
      ObjectFetchContext& context) override;

  ImmediateFuture<bool> access(RelativePath path, ObjectFetchContext& context)
      override;

  ImmediateFuture<std::string> read(
      RelativePath path,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> fileCreated(
      RelativePath relPath,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> dirCreated(
      RelativePath relPath,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> fileModified(
      RelativePath relPath,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> fileRenamed(
      RelativePath oldPath,
      RelativePath newPath,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> fileDeleted(
      RelativePath oldPath,
      ObjectFetchContext& context) override;

  ImmediateFuture<folly::Unit> dirDeleted(
      RelativePath oldPath,
      ObjectFetchContext& context) override;

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;

  // All the notifications are dispatched to this executor.
  folly::Executor::KeepAlive<folly::SequencedExecutor> notificationExecutor_;

  const std::string dotEdenConfig_;
};

} // namespace facebook::eden
