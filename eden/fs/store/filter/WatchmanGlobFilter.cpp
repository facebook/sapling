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
    folly::StringPiece filterId) {
  std::shared_ptr<ResultList> globResult = std::make_shared<ResultList>();
  auto rootId = RootId{filterId.str()};
  return store_->getRootTree(rootId, context_)
      .thenValue([this, globResult, rootId](
                     ObjectStore::GetRootTreeResult treeResult) {
        // TODO this is inefficient
        // A better idea is to have GlobBackingStore that fetches
        // glob filtered tree directly
        return root_->evaluate(
            store_,
            context_,
            RelativePathPiece{""},
            treeResult.tree,
            nullptr, // prefetch list
            *globResult,
            rootId);
      })
      .thenValue([pathCopied = path.copy(), globResult](auto&&) {
        return globResult->withRLock([pathCopied](const auto& results) {
          for (auto& result : results) {
            // TODO symlink?
            if (result.name.view() == pathCopied) {
              return false;
            }
          }
          return true;
        });
      });
}

} // namespace facebook::eden
