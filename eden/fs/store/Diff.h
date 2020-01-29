/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class Hash;
class ObjectStore;
class Tree;
class DiffContext;
class GitIgnoreStack;

/**
 * Compute the diff between two commits.
 *
 * The caller is responsible for ensuring that the ObjectStore remains valid
 * until the returned Future completes.
 *
 * The differences will be returned to the caller.
 */
folly::Future<std::unique_ptr<ScmStatus>>
diffCommitsForStatus(const ObjectStore* store, Hash hash1, Hash hash2);

/**
 * Compute the diff between a source control Tree and the current directory
 * state. This function is called with the hashes of a source control tree
 * entry and an unmaterialized inode entry.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The caller is responsible for ensuring that the context remains valid
 * until the returned Future completes.
 *
 * The differences will be recorded using the callback inside the passed
 * DiffContext.
 */
folly::Future<folly::Unit> diffTrees(
    const DiffContext* context,
    RelativePathPiece currentPath,
    Hash scmHash,
    Hash wdHash,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored);

/**
 * Process an added tree (present locally but not present in the source control
 * tree). This function is called with the hash of an unmaterialized inode
 * entry. This whole subtree is marked as added using the DiffContext.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The caller is responsible for ensuring that the context remains valid
 * until the returned Future completes.
 *
 * The differences will be recorded using the callback inside the passed
 * DiffContext.
 */
folly::Future<folly::Unit> diffAddedTree(
    const DiffContext* context,
    RelativePathPiece currentPath,
    Hash wdHash,
    const GitIgnoreStack* ignore,
    bool isIgnored);

/**
 * Process a removed tree (not present locally but present in the source control
 * tree). This function is called with the hash of the source control tree
 * entry. This whole subtree is marked as removed using the DiffContext.
 *
 * The path argument specifies the path to these trees, and will be prefixed
 * to all differences recorded in the results.
 *
 * The caller is responsible for ensuring that the context remains valid
 * until the returned Future completes.
 *
 * The differences will be recorded using the callback inside the passed
 * DiffContext.
 */
folly::Future<folly::Unit> diffRemovedTree(
    const DiffContext* context,
    RelativePathPiece currentPath,
    Hash scmHash);
} // namespace eden
} // namespace facebook
