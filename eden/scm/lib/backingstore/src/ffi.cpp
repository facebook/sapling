/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Try.h>
#include <folly/io/IOBuf.h>
#include <memory>

#include "eden/scm/lib/backingstore/include/ffi.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h" // @manual

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

void sapling_backingstore_get_blob_batch_handler(
    std::shared_ptr<GetBlobBatchResolver> resolver,
    size_t index,
    rust::String error,
    rust::Box<Blob> blob) {
  using ResolveResult = folly::Try<std::unique_ptr<folly::IOBuf>>;

  resolver->resolve(index, folly::makeTryWith([&] {
                      auto result = blob.into_raw();
                      if (result->bytes.empty()) {
                        auto box = rust::Box<Blob>::from_raw(result);
                        return ResolveResult{SaplingFetchError{error.c_str()}};
                      } else {
                        return ResolveResult{folly::IOBuf::takeOwnership(
                            reinterpret_cast<void*>(result->bytes.data()),
                            result->bytes.size(),
                            [](void* /* buf */, void* blob) mutable {
                              auto box = rust::Box<Blob>::from_raw(
                                  reinterpret_cast<Blob*>(blob));
                            },
                            reinterpret_cast<void*>(result))};
                      }
                    }));
}

} // namespace sapling
