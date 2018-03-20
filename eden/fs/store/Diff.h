/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
