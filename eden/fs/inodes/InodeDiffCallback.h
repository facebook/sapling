/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class exception_wrapper;
}

namespace facebook {
namespace eden {

class TreeEntry;

/**
 * A callback that will be invoked with results from a diff operation.
 *
 * Note that the callback functions may be invoked from multiple threads
 * simultaneously, and the callback is responsible for implementing
 * synchronization properly.
 */
class InodeDiffCallback {
 public:
  InodeDiffCallback() {}
  virtual ~InodeDiffCallback() {}

  virtual void ignoredFile(RelativePathPiece path) = 0;
  virtual void untrackedFile(RelativePathPiece path) = 0;
  virtual void removedFile(
      RelativePathPiece path,
      const TreeEntry& sourceControlEntry) = 0;
  virtual void modifiedFile(
      RelativePathPiece path,
      const TreeEntry& sourceControlEntry) = 0;

  virtual void diffError(
      RelativePathPiece path,
      const folly::exception_wrapper& ew) = 0;
};
} // namespace eden
} // namespace facebook
