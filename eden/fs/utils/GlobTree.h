/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/GlobNodeImpl.h"
#include "eden/fs/utils/GlobResult.h"

namespace facebook::eden {

class GlobTree : public GlobNodeImpl {
 public:
  // Two-parameter constructor is intended to create the root of a set of
  // globs that will be parsed into the overall glob tree.
  explicit GlobTree(bool includeDotfiles, CaseSensitivity caseSensitive)
      : GlobNodeImpl(includeDotfiles, caseSensitive) {}

  GlobTree(
      folly::StringPiece pattern,
      bool includeDotfiles,
      bool hasSpecials,
      CaseSensitivity caseSensitive)
      : GlobNodeImpl(pattern, includeDotfiles, hasSpecials, caseSensitive) {}

  /**
   * Evaluate the compiled glob against the provided Tree.
   *
   * @param store where the blobs are stored
   * @param context used for tracking
   * @param rootPath path root where glob search starts
   * @param tree metadata structure of files
   * @param fileBlobstoPrefetch a nullable list of files to fetch during
   * globbing
   */
  ImmediateFuture<folly::Unit> evaluate(
      std::shared_ptr<ObjectStore> store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      std::shared_ptr<const Tree> tree,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;
};

} // namespace facebook::eden
