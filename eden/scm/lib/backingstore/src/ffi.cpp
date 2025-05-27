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

  resolver->resolve(
      index, folly::makeTryWith([&] {
        if (error.empty()) {
          return ResolveResult{tree};
        } else {
          return ResolveResult{SaplingFetchError{std::string(error)}};
        }
      }));
}

void sapling_backingstore_get_tree_aux_batch_handler(
    std::shared_ptr<GetTreeAuxBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<TreeAuxData> aux) {
  using ResolveResult = folly::Try<std::shared_ptr<TreeAuxData>>;

  resolver->resolve(
      index, folly::makeTryWith([&] {
        if (error.empty()) {
          return ResolveResult{aux};
        } else {
          return ResolveResult{SaplingFetchError{std::string(error)}};
        }
      }));
}

void sapling_backingstore_get_blob_batch_handler(
    std::shared_ptr<GetBlobBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::unique_ptr<folly::IOBuf> blob) {
  using ResolveResult = folly::Try<std::unique_ptr<folly::IOBuf>>;

  resolver->resolve(
      index,
      folly::makeTryWith(
          [blob = std::move(blob), error = std::move(error)]() mutable {
            if (error.empty()) {
              return ResolveResult{std::move(blob)};
            } else {
              return ResolveResult{
                  SaplingFetchError{std::string(std::move(error))}};
            }
          }));
}

void sapling_backingstore_get_file_aux_batch_handler(
    std::shared_ptr<GetFileAuxBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<FileAuxData> aux) {
  using ResolveResult = folly::Try<std::shared_ptr<FileAuxData>>;

  resolver->resolve(
      index, folly::makeTryWith([&] {
        if (error.empty()) {
          return ResolveResult{aux};
        } else {
          return ResolveResult{SaplingFetchError{std::string(error)}};
        }
      }));
}

} // namespace sapling
