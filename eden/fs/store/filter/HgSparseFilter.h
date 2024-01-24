/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/container/F14Map.h>
#include <folly/logging/xlog.h>
#include <rust/cxx.h>
#include <memory>
#include <string>

#include "eden/fs/store/filter/Filter.h"
#include "eden/scm/lib/edenfs_ffi/include/ffi.h"
#include "eden/scm/lib/edenfs_ffi/src/lib.rs.h" // @manual

namespace facebook::eden {

// Extern "Rust"
struct MercurialMatcher;

class HgSparseFilter : public Filter {
 public:
  explicit HgSparseFilter(AbsolutePath checkoutPath)
      : checkoutPath_{std::move(checkoutPath)} {
    profiles_ = std::make_shared<folly::Synchronized<MercurialMatcherMap>>(
        std::in_place);
  }
  ~HgSparseFilter() override = default;

  /*
   * Checks whether a path is filtered by the given filter.
   */
  ImmediateFuture<FilterCoverage> getFilterCoverageForPath(
      RelativePathPiece path,
      folly::StringPiece filterId) const override;

 private:
  using MercurialMatcherMap = folly::
      F14NodeMap<std::string, rust::Box<facebook::eden::MercurialMatcher>>;
  std::shared_ptr<folly::Synchronized<MercurialMatcherMap>> profiles_;
  AbsolutePath checkoutPath_;
};
} // namespace facebook::eden
