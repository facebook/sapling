/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Function.h>
#include <folly/Range.h>
#include <folly/Try.h>
#include <memory>
#include <optional>
#include <string_view>

#include "eden/scm/lib/backingstore/include/BackingStoreBindings.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h" // @manual

namespace folly {
class IOBuf;
} // namespace folly

namespace sapling {

/**
 * Reference to a 20-byte hg node ID.
 *
 * In the future, should we want to continue to encode full repo paths in the
 * object ID again, this can be made into a struct.
 */
using NodeId = folly::ByteRange;

/**
 * List of NodeIds used in batch requests.
 */
using NodeIdRange = folly::Range<const NodeId*>;

/**
 * Storage for a 20-byte hg manifest id.
 */
using ManifestId = std::array<uint8_t, 20>;

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
 *   amount of time that heavyweight objects are in RAM.
 */
class SaplingNativeBackingStore {
 public:
  SaplingNativeBackingStore(
      std::string_view repository,
      const BackingStoreOptions& options);

  std::optional<ManifestId> getManifestNode(NodeId node);

  std::shared_ptr<Tree> getTree(NodeId node, bool local);

  void getTreeBatch(
      NodeIdRange requests,
      bool local,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
          resolve);

  std::unique_ptr<folly::IOBuf> getBlob(NodeId node, bool local);

  void getBlobBatch(
      NodeIdRange requests,
      bool local,
      folly::FunctionRef<
          void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)> resolve);

  std::shared_ptr<FileAuxData> getBlobMetadata(NodeId node, bool local);

  void getBlobMetadataBatch(
      NodeIdRange requests,
      bool local,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
          resolve);

  void flush();

 private:
  std::unique_ptr<sapling::BackingStore, void (*)(sapling::BackingStore*)>
      store_;
};

} // namespace sapling
