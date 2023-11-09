/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/filter/Filter.h"

namespace facebook::eden {

/**
 * A fake filter that filters if the path starts with the filter id.
 */
class FakeSubstringFilter final : public Filter {
 public:
  ~FakeSubstringFilter() override = default;

  /*
   * Checks whether a path is filtered by the given filter.
   */
  ImmediateFuture<FilterCoverage> getFilterCoverageForPath(
      RelativePathPiece path,
      folly::StringPiece filterId) const override {
    auto filterIdPos = path.view().find(filterId);

    // The filter is at the beginning of the given path
    if (filterIdPos != std::string::npos) {
      return ImmediateFuture<FilterCoverage>{
          FilterCoverage::RECURSIVELY_FILTERED};
    }

    // The filter isn't part of the path. However, a child of the path might be
    // filtered. Therefore we report UNFILTERED
    return ImmediateFuture<FilterCoverage>{FilterCoverage::UNFILTERED};

    // it's not possible for us to check if any child of the path *could* be
    // filtered because the filter can match any portion of the path
  }
};

/**
 * A fake filter that filters if the path starts with the filter id.
 */
class FakePrefixFilter final : public Filter {
 public:
  ~FakePrefixFilter() override = default;

  /*
   * Checks whether a path is filtered by the given filter.
   */
  ImmediateFuture<FilterCoverage> getFilterCoverageForPath(
      RelativePathPiece path,
      folly::StringPiece filterId) const override {
    auto filterIdSize = filterId.size();
    auto pathSize = path.view().size();
    // The filter doesn't apply to the given path because the filter is too
    // long.
    if (filterIdSize >= pathSize) {
      if (path.view().find(filterId) == 0) {
        // The FilterID begins with the path and therefore children could be
        // filtered
        return ImmediateFuture<FilterCoverage>{FilterCoverage::UNFILTERED};
      } else {
        // The path is not a substring of the FilterID and therefore does not
        // apply any of the given path's children
        return ImmediateFuture<FilterCoverage>{
            FilterCoverage::RECURSIVELY_UNFILTERED};
      }
    }

    auto filterIdPos = path.view().find(filterId);

    // The filter is at the beginning of the given path
    if (filterIdPos == 0) {
      return ImmediateFuture<FilterCoverage>{
          FilterCoverage::RECURSIVELY_FILTERED};
    }

    // The filter isn't in the path or is in the middle somewhere, therefore it
    // doesn't apply.
    return ImmediateFuture<FilterCoverage>{
        FilterCoverage::RECURSIVELY_UNFILTERED};
  }
};
} // namespace facebook::eden
