/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/GlobMatcher.h"

namespace facebook::eden {

class ObjectStore;

/**
 * This class does filter based on glob patterns
 */
class WatchmanGlobFilter : public Filter {
 public:
  explicit WatchmanGlobFilter(
      const std::vector<std::string>& globs,
      CaseSensitivity caseSensitive) {
    GlobOptions options = GlobOptions::DEFAULT;
    if (caseSensitive == CaseSensitivity::Insensitive) {
      options |= GlobOptions::CASE_INSENSITIVE;
    }
    for (auto& globStr : globs) {
      auto matcher = GlobMatcher::create(globStr, options);
      if (matcher.hasError()) {
        throw std::runtime_error(
            fmt::format("Invalid glob pattern {}", globStr));
      }
      matcher_.emplace_back(std::move(*matcher));
    }
  }

  /*
   * Check whether a path is filtered by the given filter. NOTE: this method
   * could potentially be slow. Returns a FilterCoverage enum that indicates the
   * extent of the path's filtering.
   *
   * @param filterId, we use filterId as rootId here
   */
  ImmediateFuture<FilterCoverage> getFilterCoverageForPath(
      RelativePathPiece path,
      folly::StringPiece filterId) const override;

 private:
  std::vector<GlobMatcher> matcher_;
};

} // namespace facebook::eden
