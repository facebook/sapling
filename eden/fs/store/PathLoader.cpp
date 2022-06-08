/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/PathLoader.h"
#include <vector>
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/gen-cpp2/eden_constants.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/ImmediateFuture.h"

namespace facebook::eden {

namespace {

struct ResolveTreeContext {
  std::vector<PathComponent> components;
};

ImmediateFuture<std::shared_ptr<const Tree>> resolveTree(
    std::shared_ptr<ResolveTreeContext> ctx,
    ObjectStore& objectStore,
    ObjectFetchContext& fetchContext,
    std::shared_ptr<const Tree> root,
    size_t index) {
  if (index == ctx->components.size()) {
    return std::move(root);
  }

  auto child = root->find(ctx->components[index]);
  if (child == root->end()) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        "no child with name ",
        ctx->components[index]);
  }

  if (!child->second.isTree()) {
    throw newEdenError(
        ENOTDIR,
        EdenErrorType::POSIX_ERROR,
        "child is not tree ",
        ctx->components[index]);
  }

  return objectStore.getTree(child->second.getHash(), fetchContext)
      .thenValue([ctx = std::move(ctx), &objectStore, &fetchContext, index](
                     std::shared_ptr<const Tree>&& tree) mutable {
        return resolveTree(
            ctx, objectStore, fetchContext, std::move(tree), index + 1);
      });
}

} // namespace

ImmediateFuture<std::shared_ptr<const Tree>> resolveTree(
    ObjectStore& objectStore,
    ObjectFetchContext& fetchContext,
    std::shared_ptr<const Tree> root,
    RelativePathPiece path) {
  // Don't do anything fancy with lifetimes and just get this correct as simply
  // as possible. There's room for optimization if it matters.
  auto ctx = std::make_shared<ResolveTreeContext>();
  for (auto c : path.components()) {
    ctx->components.emplace_back(c);
  }

  return resolveTree(
      std::move(ctx), objectStore, fetchContext, std::move(root), 0);
}

} // namespace facebook::eden
