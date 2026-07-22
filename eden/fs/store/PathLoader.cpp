/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/PathLoader.h"

#include "eden/fs/model/Tree.h"
#include "eden/fs/service/gen-cpp2/eden_constants.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/EdenError.h"

namespace facebook::eden {

folly::coro::now_task<std::shared_ptr<const Tree>> resolveTree(
    ObjectStore& objectStore,
    const ObjectFetchContextPtr& fetchContext,
    std::shared_ptr<const Tree> root,
    RelativePathPiece path) {
  auto tree = std::move(root);
  for (auto component : path.components()) {
    auto child = tree->find(component);
    if (child == tree->end()) {
      throw newEdenError(
          ENOENT, EdenErrorType::POSIX_ERROR, "no child with name ", component);
    }

    if (!child->second.isTree()) {
      throw newEdenError(
          ENOTDIR, EdenErrorType::POSIX_ERROR, "child is not tree ", component);
    }

    tree = co_await objectStore.co_getTree(
        child->second.getObjectId(), fetchContext);
  }
  co_return tree;
}

} // namespace facebook::eden
