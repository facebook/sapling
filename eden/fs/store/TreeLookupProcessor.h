/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class ObjectStore;
class Tree;
class TreeEntry;

class TreeLookupProcessor {
 public:
  explicit TreeLookupProcessor(
      RelativePathPiece path,
      std::shared_ptr<ObjectStore> objectStore,
      ObjectFetchContextPtr context)
      : path_{path},
        iterRange_{path_.components()},
        iter_{iterRange_.begin()},
        objectStore_{std::move(objectStore)},
        context_{std::move(context)} {}

  ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>> next(
      std::shared_ptr<const Tree> tree);

 private:
  RelativePath path_;
  RelativePath::base_type::component_iterator_range iterRange_;
  RelativePath::base_type::component_iterator iter_;
  std::shared_ptr<ObjectStore> objectStore_;
  ObjectFetchContextPtr context_;
};

/**
 * Traverse the path starting at rootTree.
 *
 * The returned variant will hold a Tree if the path refers to a directory, a
 * TreeEntry otherwise (file, symlink, etc).
 */
ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
getTreeOrTreeEntry(
    std::shared_ptr<const Tree> rootTree,
    RelativePathPiece path,
    std::shared_ptr<ObjectStore> objectStore,
    ObjectFetchContextPtr context);

} // namespace facebook::eden
