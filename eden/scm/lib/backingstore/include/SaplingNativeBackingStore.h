/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#pragma once

#include <folly/Function.h>
#include <folly/Range.h>
#include <folly/Try.h>
#include <memory>
#include <optional>
#include <string_view>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/store/ObjectFetchContext.h"
#include "eden/scm/lib/backingstore/src/ffi.rs.h"

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
using RepoPath = facebook::eden::RelativePathPiece;
using RootId = facebook::eden::RootId;
using ObjectFetchContextPtr = facebook::eden::ObjectFetchContextPtr;

struct SaplingRequest {
  // These two fields are typically borrowed from a
  // SaplingImportRequest - be cognizant of lifetimes.
  NodeId node;
  RepoPath path;

  FetchCause cause;
  ObjectFetchContextPtr context;
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
  explicit SaplingNativeBackingStore(
      std::string_view repository,
      std::string_view mount);

  std::string_view getRepoName() const {
    return repoName_;
  }

  bool dogfoodingHost() const;

  std::optional<ManifestId> getManifestNode(NodeId node);

  folly::Try<std::shared_ptr<Tree>> getTree(
      NodeId node,
      RepoPath path,
      const ObjectFetchContextPtr& context,
      sapling::FetchMode fetch_mode);

  void getTreeBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetch_mode,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<Tree>>)>
          resolve);

  folly::Try<std::shared_ptr<TreeAuxData>> getTreeAuxData(
      NodeId node,
      bool local);

  void getTreeAuxDataBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetch_mode,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<TreeAuxData>>)>
          resolve);

  folly::Try<std::unique_ptr<folly::IOBuf>> getBlob(
      NodeId node,
      RepoPath path,
      const ObjectFetchContextPtr& context,
      sapling::FetchMode fetchMode);

  void getBlobBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetchMode,
      bool allowIgnoreResult,
      folly::FunctionRef<
          void(size_t, folly::Try<std::unique_ptr<folly::IOBuf>>)> resolve);

  folly::Try<std::shared_ptr<FileAuxData>> getBlobAuxData(
      NodeId node,
      bool local);

  void getBlobAuxDataBatch(
      SaplingRequestRange requests,
      sapling::FetchMode fetch_mode,
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<FileAuxData>>)>
          resolve);

  folly::Try<std::shared_ptr<GlobFilesResponse>> getGlobFiles(
      std::string_view commit_id,
      const std::vector<std::string>& suffixes,
      const std::vector<std::string>& prefixes);

  void workingCopyParentHint(const RootId& parent);

  void flush();

 private:
  std::unique_ptr<sapling::BackingStore, void (*)(sapling::BackingStore*)>
      store_;
  std::string repoName_;
};

} // namespace sapling
