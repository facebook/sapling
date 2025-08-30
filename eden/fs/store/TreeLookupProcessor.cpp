/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeLookupProcessor.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"

namespace facebook::eden {

ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
TreeLookupProcessor::next(std::shared_ptr<const Tree> tree) {
  using RetType = std::variant<std::shared_ptr<const Tree>, TreeEntry>;
  if (iter_ == iterRange_.end()) {
    return RetType{tree};
  }

  auto name = *iter_++;
  auto it = tree->find(name);

  if (it == tree->cend()) {
    return makeImmediateFuture<RetType>(
        std::system_error(ENOENT, std::generic_category()));
  }

  if (iter_ == iterRange_.end()) {
    if (it->second.isTree()) {
      return objectStore_->getTree(it->second.getObjectId(), context_)
          .thenValue(
              [](std::shared_ptr<const Tree> tree) -> RetType { return tree; });
    } else {
      return RetType{it->second};
    }
  } else {
    if (!it->second.isTree()) {
      return makeImmediateFuture<RetType>(
          std::system_error(ENOTDIR, std::generic_category()));
    } else {
      return objectStore_->getTree(it->second.getObjectId(), context_)
          .thenValue([this](std::shared_ptr<const Tree> tree) {
            return next(std::move(tree));
          });
    }
  }
}

ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
getTreeOrTreeEntry(
    std::shared_ptr<const Tree> rootTree,
    RelativePathPiece path,
    std::shared_ptr<ObjectStore> objectStore,
    ObjectFetchContextPtr context) {
  if (path.empty()) {
    return std::variant<std::shared_ptr<const Tree>, TreeEntry>{
        std::move(rootTree)};
  }

  auto processor = std::make_unique<TreeLookupProcessor>(
      path, std::move(objectStore), context.copy());
  auto future = processor->next(std::move(rootTree));
  return std::move(future).ensure([p = std::move(processor)] {});
}

} // namespace facebook::eden
