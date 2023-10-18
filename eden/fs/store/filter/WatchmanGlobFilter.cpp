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

ImmediateFuture<bool> WatchmanGlobFilter::isPathFiltered(
    RelativePathPiece path,
    folly::StringPiece) const {
  for (const auto& matcher : matcher_) {
    if (matcher.match(path.view())) {
      return false;
    }
  }
  return true;
}

} // namespace facebook::eden
