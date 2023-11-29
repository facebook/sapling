/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <rust/cxx.h>
#include <memory>

namespace sapling {

class SaplingFetchError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

struct Tree;

/**
 * Resolver used in the processing of getTreeBatch requests.
 */
struct GetTreeBatchResolver {
  explicit GetTreeBatchResolver(
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
          resolve)
      : resolve{std::move(resolve)} {}

  folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)> resolve;
};

void sapling_backingstore_get_tree_batch_handler(
    std::shared_ptr<GetTreeBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<Tree> tree);

} // namespace sapling
