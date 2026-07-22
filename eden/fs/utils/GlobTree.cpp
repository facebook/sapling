/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GlobTree.h"

#include <iomanip>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/store/ObjectStore.h"

using folly::StringPiece;

namespace facebook::eden {

folly::coro::now_task<folly::Unit> GlobTree::evaluate(
    std::shared_ptr<ObjectStore> store,
    const ObjectFetchContextPtr& context,
    RelativePathPiece rootPath,
    std::shared_ptr<const Tree> tree,
    PrefetchList* fileBlobsToPrefetch,
    ResultList* globResult,
    const RootId& originRootId) const {
  co_return co_await evaluateImpl<
      GlobNodeImpl::TreeRoot,
      GlobNodeImpl::TreeRootPtr>(
      store.get(),
      context,
      rootPath,
      GlobNodeImpl::TreeRoot(std::move(tree)),
      fileBlobsToPrefetch,
      globResult,
      originRootId);
}

} // namespace facebook::eden
