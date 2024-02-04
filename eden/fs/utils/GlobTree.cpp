/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "GlobTree.h"

#include <iomanip>

using folly::StringPiece;

namespace facebook::eden {

ImmediateFuture<folly::Unit> GlobTree::evaluate(
    std::shared_ptr<ObjectStore> store,
    const ObjectFetchContextPtr& context,
    RelativePathPiece rootPath,
    std::shared_ptr<const Tree> tree,
    PrefetchList* fileBlobsToPrefetch,
    ResultList& globResult,
    const RootId& originRootId) const {
  return evaluateImpl<GlobNodeImpl::TreeRoot, GlobNodeImpl::TreeRootPtr>(
             store.get(),
             context,
             rootPath,
             GlobNodeImpl::TreeRoot(std::move(tree)),
             fileBlobsToPrefetch,
             globResult,
             originRootId)
      // Make sure the store stays alive for the duration of globbing.
      .ensure([store] {});
}

} // namespace facebook::eden
