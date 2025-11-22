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
 * Helper function to strip version prefix from filterId.
 * FilterIds can have a version prefix like "V1:", "Legacy:", "V2:", etc.
 */
inline folly::StringPiece stripVersionPrefix(folly::StringPiece filterId) {
  auto colonPos = filterId.find(':');
  if (colonPos != std::string::npos) {
    return filterId.subpiece(colonPos + 1);
  }
  // No version prefix found
  return filterId;
}

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
    auto actualFilterId = stripVersionPrefix(filterId);
    auto filterIdPos = path.view().find(actualFilterId);

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

  bool areFiltersIdentical(folly::StringPiece lhs, folly::StringPiece rhs)
      const override {
    return stripVersionPrefix(lhs) == stripVersionPrefix(rhs);
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
    auto actualFilterId = stripVersionPrefix(filterId);
    auto filterIdSize = actualFilterId.size();
    auto pathSize = path.view().size();
    // The filter doesn't apply to the given path because the filter is too
    // long.
    if (filterIdSize >= pathSize) {
      if (path.view().find(actualFilterId) == 0) {
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

    auto filterIdPos = path.view().find(actualFilterId);

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

  bool areFiltersIdentical(folly::StringPiece lhs, folly::StringPiece rhs)
      const override {
    return stripVersionPrefix(lhs) == stripVersionPrefix(rhs);
  }
};
} // namespace facebook::eden
