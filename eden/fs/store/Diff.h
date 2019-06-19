/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/service/gen-cpp2/eden_types.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {

class Hash;
class ObjectStore;
class Tree;

/**
 * Compute the diff between two commits.
 *
 * The caller is responsible for ensuring that the ObjectStore remains valid
 * until the returned Future completes.
 */
folly::Future<ScmStatus>
diffCommits(ObjectStore* store, Hash commit1, Hash commit2);

/**
 * Compute the diff between two commits.
 *
 * The caller is responsible for ensuring that the ObjectStore remains valid
 * until the returned Future completes.
 */
folly::Future<ScmStatus> diffTrees(ObjectStore* store, Hash tree1, Hash tree2);
folly::Future<ScmStatus>
diffTrees(ObjectStore* store, const Tree& tree1, const Tree& tree2);

} // namespace eden
} // namespace facebook
