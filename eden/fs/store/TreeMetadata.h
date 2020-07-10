/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

namespace facebook {
namespace eden {

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
  using HashIndexedEntryMetadata = std::vector<std::pair<Hash, BlobMetadata>>;

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
  size_t getSerializedSize() const;

  size_t getNumberOfEntries() const;

  static constexpr size_t ENTRY_SIZE =
      Hash::RAW_SIZE + SerializedBlobMetadata::SIZE;

  EntryMetadata entryMetadata_;
};

} // namespace eden
} // namespace facebook
