/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class ObjectFetchContext;
class ObjectStore;
class Tree;

ImmediateFuture<std::shared_ptr<const Tree>> resolveTree(
    ObjectStore& objectStore,
    ObjectFetchContext& fetchContext,
    std::shared_ptr<const Tree> root,
    RelativePathPiece path);

} // namespace facebook::eden
