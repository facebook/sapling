/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <memory>
#include <string_view>

#include "eden/scm/lib/backingstore/c_api/BackingStoreBindings.h"

namespace folly {
class IOBuf;
} // namespace folly

namespace sapling {

/**
 * Provides a type-safe layer and a more convenient API around the raw
 * BackingStoreBindings.h C functions.
 *
 * Rather than individually documenting each method, the overall design is
 * described here:
 *
 * - If `local` is true, only disk caches are queried.
 * - If the object is not found, the error is logged and nullptr is returned.
 * - Batch methods take a callback function which is evaluated once per
 *   returned result. Compared to returning a vector, this minimizes the
 *   amount of time that heavyweight are in RAM.
 */
class SaplingNativeBackingStore {
 public:
  SaplingNativeBackingStore(
      std::string_view repository,
      const BackingStoreOptions& options);

  std::shared_ptr<Tree> getTree(folly::ByteRange node, bool local);

  void getTreeBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::shared_ptr<Tree>)>&& resolve);

  std::unique_ptr<folly::IOBuf>
  getBlob(folly::ByteRange name, folly::ByteRange node, bool local);

  void getBlobBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::unique_ptr<folly::IOBuf>)>&& resolve);

  std::shared_ptr<FileAuxData> getBlobMetadata(
      folly::ByteRange node,
      bool local);

  void getBlobMetadataBatch(
      const std::vector<std::pair<folly::ByteRange, folly::ByteRange>>&
          requests,
      bool local,
      std::function<void(size_t, std::shared_ptr<FileAuxData>)>&& resolve);

  void flush();

 private:
  sapling::CFallible<
      sapling::BackingStore,
      sapling::sapling_backingstore_free>::Ptr store_;
};

} // namespace sapling
