/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/EdenError.h"

#include "eden/scm/lib/edenfs_ffi/include/ffi.h"
#include "eden/scm/lib/edenfs_ffi/src/lib.rs.h" // @manual

namespace facebook::eden {

/**
 * This class does filter based on glob patterns
 */
class GlobFilter : public Filter {
 public:
  explicit GlobFilter(
      const std::vector<std::string>& globs,
      const CaseSensitivity caseSensitive) {
    rust::Vec<rust::String> rustGlobs;
    for (const auto& glob : globs) {
      rustGlobs.push_back(rust::String{glob});
    }
    // MatcherWrapper is the tunnel we use to communicate to rust to create
    // TreeMatcher struct
    // Note we use this method so that we don't need to expose complex struct
    // TreeMatcher to cpp
    std::shared_ptr<MatcherWrapper> wrapper =
        std::make_shared<MatcherWrapper>();
    create_tree_matcher(
        rustGlobs, caseSensitive == CaseSensitivity::Sensitive, wrapper);
    if (!wrapper->error_.empty()) {
      // matcher creation failed, throw the error
      throw newEdenError(
          EdenErrorType::ARGUMENT_ERROR, wrapper->error_.c_str());
    }
    if (wrapper->matcher_ == nullptr) {
      // neither matcher_ and error_ is set, this should never happen
      throw newEdenError(
          EdenErrorType::GENERIC_ERROR,
          "Failed to create TreeMatcher, rust returned nullptr");
    }
    matcher_ = std::move(wrapper->matcher_);
  } // namespace facebook::eden

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
  std::unique_ptr<rust::Box<facebook::eden::MercurialMatcher>> matcher_;
};

} // namespace facebook::eden
