/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/filter/WatchmanGlobFilter.h"
#include "eden/fs/utils/GlobResult.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

ImmediateFuture<FilterCoverage> WatchmanGlobFilter::getFilterCoverageForPath(
    RelativePathPiece path,
    folly::StringPiece) const {
  for (const auto& matcher : matcher_) {
    if (matcher.match(path.view())) {
      return FilterCoverage::UNFILTERED;
    }
  }
  return FilterCoverage::RECURSIVELY_FILTERED;
}

} // namespace facebook::eden
