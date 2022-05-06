/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class exception_wrapper;
}

namespace facebook::eden {

class TreeEntry;

/**
 * A callback that will be invoked with results from a diff operation.
 *
 * Note that the callback functions may be invoked from multiple threads
 * simultaneously, and the callback is responsible for implementing
 * synchronization properly.
 */
class DiffCallback {
 public:
  DiffCallback() {}
  virtual ~DiffCallback() {}

  virtual void ignoredPath(RelativePathPiece path, dtype_t type) = 0;
  virtual void addedPath(RelativePathPiece path, dtype_t type) = 0;
  virtual void removedPath(RelativePathPiece path, dtype_t type) = 0;
  virtual void modifiedPath(RelativePathPiece path, dtype_t type) = 0;

  virtual void diffError(
      RelativePathPiece path,
      const folly::exception_wrapper& ew) = 0;
};

} // namespace facebook::eden
