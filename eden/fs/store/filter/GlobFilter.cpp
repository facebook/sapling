/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/filter/GlobFilter.h"
#include "eden/fs/utils/ImmediateFuture.h"

#include "eden/scm/lib/edenfs_ffi/src/lib.rs.h" // @manual

namespace facebook::eden {

ImmediateFuture<FilterCoverage> GlobFilter::getFilterCoverageForPath(
    RelativePathPiece path,
    folly::StringPiece) const {
  return makeImmediateFutureWith([path = std::move(path), this] {
    auto filterResult =
        (*matcher_)->matches_directory(rust::Str{path.asString()});
    switch (filterResult) {
      case FilterDirectoryMatch::RecursivelyUnfiltered:
        return FilterCoverage::RECURSIVELY_UNFILTERED;
      case FilterDirectoryMatch::RecursivelyFiltered:
        return FilterCoverage::RECURSIVELY_FILTERED;
      case FilterDirectoryMatch::Unfiltered:
        return FilterCoverage::UNFILTERED;
      default:
        throwf<std::invalid_argument>(
            "Rust returned an invalid filter FilterDirectoryMatch result: {}",
            static_cast<uint8_t>(filterResult));
    }
  });
}

} // namespace facebook::eden
