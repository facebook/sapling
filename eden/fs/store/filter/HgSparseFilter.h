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
#include "eden/scm/lib/edenfs-ffi/src/ffi.h"
#include "eden/scm/lib/edenfs-ffi/src/lib.rs.h" // @manual

namespace facebook::eden {

// Extern "Rust"
struct SparseProfileRoot;

class HgSparseFilter : public Filter {
 public:
  explicit HgSparseFilter(AbsolutePath checkoutPath)
      : checkoutPath_{std::move(checkoutPath)} {
    profiles_ =
        std::make_shared<folly::Synchronized<SparseMatcherMap>>(std::in_place);
  }
  ~HgSparseFilter() override {}

  /*
   * Checks whether a path is filtered by the given filter.
   */
  ImmediateFuture<bool> isPathFiltered(
      RelativePathPiece path,
      folly::StringPiece filterId) const override;

 private:
  // TODO(cuev): We may want to use a F14FastMap instead since it doesn't matter
  // if the string or rust::Box are moved. We'll hold off on investigating for
  // now since in the future we may store a Matcher in the map instead of a
  // SparseProfileRoot object. See fbcode/folly/container/F14.md for more info.
  using SparseMatcherMap = folly::
      F14NodeMap<std::string, rust::Box<facebook::eden::SparseProfileRoot>>;
  std::shared_ptr<folly::Synchronized<SparseMatcherMap>> profiles_;
  AbsolutePath checkoutPath_;
};
} // namespace facebook::eden
