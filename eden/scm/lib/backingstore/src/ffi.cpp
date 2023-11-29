/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/scm/lib/backingstore/include/ffi.h"

namespace sapling {

void sapling_backingstore_get_tree_batch_handler(
    std::shared_ptr<GetTreeBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<Tree> tree) {
  using ResolveResult = folly::Try<std::shared_ptr<Tree>>;

  resolver->resolve(index, folly::makeTryWith([&] {
                      if (tree) {
                        return ResolveResult{tree};
                      } else {
                        return ResolveResult{SaplingFetchError{error.c_str()}};
                      }
                    }));
}

} // namespace sapling
