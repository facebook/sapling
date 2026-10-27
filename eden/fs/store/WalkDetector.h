/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

/**
 * Walk detection for one mount. EdenFS reports the mount's backing store
 * fetches and its working-copy parent here, and the backing store's walk
 * detector turns crawls of the mount into prefetches. A BackingStore that
 * does not detect walks creates no WalkDetector, and neither does a mount
 * created with `experimental:enable-walk-detector` off, so holders check for
 * null.
 */
class WalkDetector {
 public:
  virtual ~WalkDetector() = default;

  /**
   * A blob fetched from the backing store. `origin` says whether it came from
   * the backing store's local cache or from the network. `parentCounts` are
   * the entry counts of the file's directory when the caller knows them.
   */
  virtual void witnessFileRead(
      const ObjectId& id,
      ObjectFetchContext::Origin origin,
      OptionalProcessId pid,
      std::optional<ObjectFetchContext::DirEntryCounts> parentCounts) = 0;

  /**
   * A tree fetched from the backing store.
   */
  virtual void witnessTreeRead(
      const ObjectId& id,
      const Tree& tree,
      ObjectFetchContext::Origin origin,
      OptionalProcessId pid) = 0;

  /**
   * The entry counts of a directory whose inode was loaded, however its
   * contents arrived.
   */
  virtual void witnessDirectorySize(
      RelativePathPiece path,
      ObjectFetchContext::DirEntryCounts counts) = 0;

  /**
   * The mount's working-copy parent commit, at mount time and after every
   * checkout.
   */
  virtual void workingCopyParentHint(const RootId& parent) = 0;
};

} // namespace facebook::eden
