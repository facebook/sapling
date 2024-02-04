/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/String.h"

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

  ImmediateFuture<bool> isFinalSymlinkPathDirectory(
      RelativePath symlink,
      string_view targetStringView,
      const ObjectFetchContextPtr& context,
      const int remainingRecursionDepth = kMaxSymlinkChainDepth);

  std::variant<AbsolutePath, RelativePath> determineTargetType(
      RelativePath symlink,
      string_view targetStringView);

  // Method that can be used for normalizing all the parts of a path containing
  // symlinks. For instance, if the directory structure looks somewhat like
  // this:
  // ```
  // ├── a -> b/y
  // ├── b -> x
  // └── x
  //     └── y
  //         └── z
  // ```
  // Then calling `resolveSymlinkPath("a", mycontext)` will return `x/y`.
  // This is particularly useful when trying to determine whether a target is a
  // file or a directory, thus creating the proper symlink type.
  ImmediateFuture<std::variant<AbsolutePath, RelativePath>> resolveSymlinkPath(
      RelativePath path,
      const ObjectFetchContextPtr& context,
      const size_t remainingRecursionDepth = kMaxSymlinkChainDepth);

 private:
  // The EdenMount associated with this dispatcher.
  EdenMount* const mount_;
  folly::Synchronized<std::unordered_set<RelativePath>> symlinkCheck_;

  const std::string dotEdenConfig_;

  bool symlinksEnabled_;

  ImmediateFuture<std::variant<AbsolutePath, RelativePath>>
  resolveSymlinkPathImpl(
      RelativePath path,
      const ObjectFetchContextPtr& context,
      std::vector<RelativePath> pathParts,
      const size_t solvedLen,
      const size_t remainingRecursionDepth);
};

} // namespace facebook::eden
