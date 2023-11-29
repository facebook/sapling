/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <rust/cxx.h>

namespace sapling {

class SaplingFetchError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

struct Tree;
struct Blob;
struct FileAuxData;

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

/**
 * Resolver used in the processing of getBlobBatch requests.
 */
struct GetBlobBatchResolver {
  explicit GetBlobBatchResolver(
      folly::FunctionRef<
          void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)> resolve)
      : resolve{std::move(resolve)} {}

  folly::FunctionRef<void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)>
      resolve;
};

/**
 * Resolver used in the processing of getBlobMetadataBatch requests.
 */
struct GetFileAuxBatchResolver {
  explicit GetFileAuxBatchResolver(
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
          resolve)
      : resolve{std::move(resolve)} {}

  folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
      resolve;
};

void sapling_backingstore_get_tree_batch_handler(
    std::shared_ptr<GetTreeBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<Tree> tree);

void sapling_backingstore_get_blob_batch_handler(
    std::shared_ptr<GetBlobBatchResolver> resolver,
    size_t index,
    rust::String error,
    rust::Box<Blob> blob);

void sapling_backingstore_get_file_aux_batch_handler(
    std::shared_ptr<GetFileAuxBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<FileAuxData> aux);

} // namespace sapling
