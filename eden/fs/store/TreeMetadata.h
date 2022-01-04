/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <variant>
#include <vector>

#include <folly/io/IOBuf.h>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/SerializedBlobMetadata.h"

namespace facebook::eden {

class BlobMetadata;
class StoreResult;

/**
 * This is to help manipulate and store the metadata for the blob entries
 * a tree. Currently "metadata" means the size and the SHA-1 hash of a Blob's
 * contents.
 */
class TreeMetadata {
 public:
  /** Used to prepare the tree metadata for storage and when tree metadata is
   * read out of the local store. --
   * Storing tree metadata indexed by hashes instead of names removes the
   * complexity of storing variable length names. It also allows us to easily
   * Store BlobMetadata from stored TreeMetadata since BlobMetadata is stored
   * under the eden hash for a blob.
   */
  using HashIndexedEntryMetadata =
      std::vector<std::pair<ObjectId, BlobMetadata>>;

  /** Used when TreeMetdata was just fethed from the server --
   * the server is unaware of the eden specific hashes we use in eden, so
   * tree metdata from the server will use names to index the metdata for the
   * entries in the tree.
   */
  using NameIndexedEntryMetadata =
      std::vector<std::pair<std::string, BlobMetadata>>;

  using EntryMetadata =
      std::variant<HashIndexedEntryMetadata, NameIndexedEntryMetadata>;

  explicit TreeMetadata(EntryMetadata entryMetadata);

  /**
   * Serializes the metadata for all of the blob entries in the tree.
   *
   * note: hashes of each of the entries are used in serialization, so each of
   * the EntryIdentifier for the entries must contain the hash of the entry
   * before calling this method. Otherwise this raises a std::domain_error.
   */
  folly::IOBuf serialize() const;

  static TreeMetadata deserialize(const StoreResult& result);

  const EntryMetadata& entries() const {
    return entryMetadata_;
  }

 private:
  size_t getNumberOfEntries() const;

  folly::IOBuf serializeV1() const;

  folly::IOBuf serializeV2(size_t serialized_size) const;

  static TreeMetadata deserializeV1(
      const folly::StringPiece data,
      uint32_t numberOfEntries);

  static TreeMetadata deserializeV2(
      const folly::StringPiece data,
      uint32_t numberOfEntries);

  static constexpr size_t ENTRY_SIZE_V1 =
      Hash20::RAW_SIZE + SerializedBlobMetadata::SIZE;

  static constexpr uint32_t SERIALIZED_V2_MARKER = 1u << 31;
  static constexpr uint32_t V2_VERSION = 2u;

  EntryMetadata entryMetadata_;
};

} // namespace facebook::eden
