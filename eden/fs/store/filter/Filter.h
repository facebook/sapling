/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

// A null filter indicates that nothing should be filtered (i.e. no filter is
// applied to the repo).
constexpr const char kNullFilterId[] = "null";

namespace facebook::eden {

class Filter {
 public:
  virtual ~Filter() {}

  /*
   * Checks whether a path is filtered by the given filter.
   */
  virtual ImmediateFuture<bool> isPathFiltered(
      RelativePathPiece path,
      folly::StringPiece filterId) const = 0;
};
} // namespace facebook::eden
