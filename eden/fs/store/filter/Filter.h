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

enum FilterCoverage {
  /**
   * The filter applies to the given path (and therefore all its children).
   */
  RECURSIVELY_FILTERED,

  /**
   * The filter doesn't apply to the given path or any of its children.
   */
  RECURSIVELY_UNFILTERED,

  /**
   * The filter doesn't apply to the given path BUT it may apply to children.
   */
  UNFILTERED,
};

class Filter {
 public:
  virtual ~Filter() = default;

  /*
   * Returns a FilterCoverage struct that indicates whether the filter applies
   * to the given path or any of its children. NOTE: FilterCoverage::UNFILTERED
   * does NOT mean that no children are filtered. It simply means that the given
   * path is not filtered, but it may have children that are filtered.
   */
  virtual ImmediateFuture<FilterCoverage> getFilterCoverageForPath(
      RelativePathPiece path,
      folly::StringPiece filterId) const = 0;
};
} // namespace facebook::eden
