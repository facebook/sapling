/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/coro/safe/NowTask.h>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class ObjectStore;
class Tree;
class TreeEntry;

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

folly::coro::now_task<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
co_getTreeOrTreeEntry(
    std::shared_ptr<const Tree> rootTree,
    RelativePathPiece path,
    std::shared_ptr<ObjectStore> objectStore,
    ObjectFetchContextPtr context);

} // namespace facebook::eden
