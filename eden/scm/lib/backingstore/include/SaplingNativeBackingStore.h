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

#include "eden/fs/store/ObjectFetchContext.h"
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
using FetchCause = facebook::eden::ObjectFetchContext::Cause;

struct SaplingRequest {
  NodeId node;
  FetchCause cause;
  // TODO: sapling::FetchMode mode;
  // TODO: sapling::ClientRequestInfo cri;
};

/**
 * List of SaplingRequests used in batch requests.
 */
using SaplingRequestRange = folly::Range<const SaplingRequest*>;

/**
 * Storage for a 20-byte hg manifest id.
 */
using ManifestId = std::array<uint8_t, 20>;

/**
 * Provides a type-safe layer and a more convenient API around the ffi C/C++
 * functions.
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
      const SaplingNativeBackingStoreOptions& options);

  std::string_view getRepoName() const {
    return repoName_;
  }

  std::optional<ManifestId> getManifestNode(NodeId node);

  folly::Try<std::shared_ptr<Tree>> getTree(
      NodeId node,
      sapling::FetchMode fetch_mode);

  void getTreeBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetch_mode,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
          resolve);

  folly::Try<std::unique_ptr<folly::IOBuf>> getBlob(
      NodeId node,
      sapling::FetchMode fetchMode);

  void getBlobBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetchMode,
      folly::FunctionRef<
          void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)> resolve);

  folly::Try<std::shared_ptr<FileAuxData>> getBlobMetadata(
      NodeId node,
      bool local);

  void getBlobMetadataBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetch_mode,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
          resolve);

  void flush();

 private:
  std::unique_ptr<sapling::BackingStore, void (*)(sapling::BackingStore*)>
      store_;
  std::string repoName_;
};

} // namespace sapling
