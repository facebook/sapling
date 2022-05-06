/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <iosfwd>

#include <folly/Synchronized.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/DiffCallback.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook::eden {

class ScmStatusDiffCallback : public DiffCallback {
 public:
  void ignoredPath(RelativePathPiece path, dtype_t type) override;
  void addedPath(RelativePathPiece path, dtype_t type) override;
  void removedPath(RelativePathPiece path, dtype_t type) override;
  void modifiedPath(RelativePathPiece path, dtype_t type) override;

  void diffError(RelativePathPiece path, const folly::exception_wrapper& ew)
      override;

  /**
   * Extract the ScmStatus object from this callback.
   *
   * This method should be called no more than once, as this destructively
   * moves the results out of the callback.  It should only be invoked after
   * the diff operation has completed.
   */
  ScmStatus extractStatus();

 private:
  folly::Synchronized<ScmStatus> data_;
};

/**
 * Returns the single-char representation for the ScmFileStatus used by
 * SCMs such as Git and Mercurial.
 */
char scmStatusCodeChar(ScmFileStatus code);

std::ostream& operator<<(std::ostream& os, const ScmStatus& status);

} // namespace facebook::eden
