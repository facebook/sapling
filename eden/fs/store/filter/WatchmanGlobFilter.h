/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <eden/fs/store/BackingStore.h>
#include <eden/fs/utils/CaseSensitivity.h>
#include "eden/fs/store/filter/Filter.h"
#include "eden/fs/utils/GlobTree.h"

namespace facebook::eden {

class ObjectStore;

/**
 * This class does filter based on glob patterns
 */
class WatchmanGlobFilter : public Filter {
 public:
  explicit WatchmanGlobFilter(
      const std::vector<std::string>& globs,
      std::shared_ptr<ObjectStore> store,
      const ObjectFetchContextPtr& context,
      CaseSensitivity caseSensitive)
      : store_{std::move(store)},
        context_{context.copy()},
        root_{std::make_shared<GlobTree>(true, caseSensitive)} {
    for (auto& globStr : globs) {
      root_->parse(globStr);
    }
  }

  /*
   * Check whether a path is filtered by the given filter.
   * Note this method could potentially slow
   * Returns true if the path is filtered out by globs
   *
   * @param filterId, we use filterId as rootId here
   */
  ImmediateFuture<bool> isPathFiltered(
      RelativePathPiece path,
      folly::StringPiece filterId) override;

 private:
  std::shared_ptr<ObjectStore> store_;
  const ObjectFetchContextPtr context_;

  std::shared_ptr<GlobTree> root_;

  // TODO: we likely need to cache compiled regexes somewhere
};

} // namespace facebook::eden
