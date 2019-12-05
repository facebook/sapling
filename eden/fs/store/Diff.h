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
class DiffContext;

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

} // namespace eden
} // namespace facebook
