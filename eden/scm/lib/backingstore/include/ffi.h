/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/container/FBVector.h>
#include <folly/futures/Future.h>
#include <rust/cxx.h>
#include <optional>
#include <string_view>
#include <utility>

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/model/TreeAuxDataFwd.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/TreeFwd.h"
#include "eden/fs/store/hg/HgProxyHash.h"

namespace sapling {

class SaplingFetchError : public std::runtime_error {
 public:
  using std::runtime_error::runtime_error;
};

struct TreeAuxData;
struct Blob;
struct FileAuxData;
class TreeBuilder;

/**
 * Resolver used in the processing of getTreeBatch requests.
 */
struct GetTreeBatchResolver {
  explicit GetTreeBatchResolver(
      folly::FunctionRef<void(size_t, folly::Try<facebook::eden::TreePtr>)>
          resolve)
      : resolve{std::move(resolve)} {}

  folly::FunctionRef<void(size_t, folly::Try<facebook::eden::TreePtr>)> resolve;
};

/**
 * Resolver used in the processing of getTreeAuxDataBatch requests.
 */
struct GetTreeAuxBatchResolver {
  explicit GetTreeAuxBatchResolver(
      folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<TreeAuxData>>)>
          resolve)
      : resolve{std::move(resolve)} {}

  folly::FunctionRef<void(size_t, folly::Try<std::shared_ptr<TreeAuxData>>)>
      resolve;
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
 * Resolver used in the processing of getBlobAuxDataBatch requests.
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
    std::unique_ptr<TreeBuilder> builder);

void sapling_backingstore_get_tree_aux_batch_handler(
    std::shared_ptr<GetTreeAuxBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<TreeAuxData> aux);

void sapling_backingstore_get_blob_batch_handler(
    std::shared_ptr<GetBlobBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::unique_ptr<folly::IOBuf> blob);

void sapling_backingstore_get_file_aux_batch_handler(
    std::shared_ptr<GetFileAuxBatchResolver> resolver,
    size_t index,
    rust::String error,
    std::shared_ptr<FileAuxData> aux);

// Helper so that Rust can construct an eden Tree "natively" with no
// intermediate objects.
class TreeBuilder {
 public:
  explicit TreeBuilder(
      facebook::eden::ObjectId oid,
      facebook::eden::RelativePathPiece path,
      facebook::eden::CaseSensitivity caseSensitive,
      facebook::eden::HgObjectIdFormat objectIdFormat)
      : oid_{std::move(oid)},
        path_{path},
        caseSensitive_{caseSensitive},
        objectIdFormat_{objectIdFormat} {}

  // Add tree entry (no aux data available).
  void add_entry(
      rust::Str name,
      const std::array<uint8_t, 20>& hg_node,
      facebook::eden::TreeEntryType ttype);

  // Add tree entry with aux data.
  void add_entry_with_aux_data(
      rust::Str name,
      const std::array<uint8_t, 20>& hg_node,
      facebook::eden::TreeEntryType ttype,
      const uint64_t size,
      const std::array<uint8_t, 20>& sha1,
      const std::array<uint8_t, 32>& blake3);

  // Set aux data for tree itself (if available).
  void set_aux_data(const std::array<uint8_t, 32>& digest, uint64_t size);

  // Reserve space in vector for `size` entries.
  void reserve(size_t size) {
    entries_.reserve(size);
  }

  // Mark tree as "missing", causing `build()` to return `nullptr`.
  void mark_missing() {
    missing_ = true;
  }

  // Number of file entries added so far.
  size_t num_files() const {
    return numFiles_;
  }

  // Number of dir entries added so far.
  size_t num_dirs() const {
    return numDirs_;
  }

  // Construct the Tree.
  facebook::eden::TreePtr build();

 private:
  // Emplace entry into our vector and perform some bookkeeping.
  void emplace_entry(rust::Str name, facebook::eden::TreeEntry&& entry);

  // Construct oid for an entry with given name.
  facebook::eden::ObjectId make_entry_oid(
      const std::array<uint8_t, 20>& hg_node,
      rust::Str name);

  folly::fbvector<
      std::pair<facebook::eden::PathComponent, facebook::eden::TreeEntry>>
      entries_;
  facebook::eden::ObjectId oid_;
  facebook::eden::RelativePathPiece path_;
  facebook::eden::TreeAuxDataPtr auxData_;
  facebook::eden::CaseSensitivity caseSensitive_;
  facebook::eden::HgObjectIdFormat objectIdFormat_;
  bool missing_ = false;
  size_t numFiles_ = 0;
  size_t numDirs_ = 0;
};

std::unique_ptr<TreeBuilder> new_builder(
    bool caseSensitive,
    facebook::eden::HgObjectIdFormat oidFormat,
    const rust::Slice<const uint8_t> oid,
    const rust::Slice<const uint8_t> path);

} // namespace sapling
