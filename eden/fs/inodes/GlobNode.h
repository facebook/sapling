/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/utils/GlobNodeImpl.h"
#include "eden/fs/utils/GlobResult.h"

namespace facebook::eden {

/**
 * This class does globbing with inode, for class that does globbing with tree
 * metadata see GlobTree
 */
class GlobNode : public GlobNodeImpl {
 public:
  // Two-parameter constructor is intended to create the root of a set of
  // globs that will be parsed into the overall glob tree.
  explicit GlobNode(bool includeDotfiles, CaseSensitivity caseSensitive)
      : GlobNodeImpl(includeDotfiles, caseSensitive) {}

  using PrefetchList = folly::Synchronized<std::vector<ObjectId>>;

  GlobNode(
      folly::StringPiece pattern,
      bool includeDotfiles,
      bool hasSpecials,
      CaseSensitivity caseSensitive)
      : GlobNodeImpl(pattern, includeDotfiles, hasSpecials, caseSensitive) {}

  /**
   * Evaluate the compiled glob against the provided TreeInode and path.
   *
   * The results are appended to the globResult list which the caller is
   * responsible for ensuring that its lifetime will exceed the lifetime of the
   * returned ImmediateFuture.
   *
   * When fileBlobsToPrefetch is non-null, the Hash of the globbed files will
   * be appended to it.
   */
  ImmediateFuture<folly::Unit> evaluate(
      std::shared_ptr<ObjectStore> store,
      const ObjectFetchContextPtr& context,
      RelativePathPiece rootPath,
      TreeInodePtr root,
      PrefetchList* fileBlobsToPrefetch,
      ResultList& globResult,
      const RootId& originRootId) const;
};

} // namespace facebook::eden
