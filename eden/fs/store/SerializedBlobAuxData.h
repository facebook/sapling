/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include <folly/Range.h>

#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook::eden {

class SerializedBlobAuxData {
 public:
  explicit SerializedBlobAuxData(const BlobAuxData& auxData);
  SerializedBlobAuxData(
      const Hash20& sha1,
      const std::optional<Hash32>& blake3,
      uint64_t blobSize);
  folly::ByteRange slice() const;

  static BlobAuxDataPtr parse(
      const ObjectId& blobID,
      const StoreResult& result);

 private:
  void serialize(
      const Hash20& sha1,
      const std::optional<Hash32>& blake3,
      uint64_t blobSize);

  /**
   * The serialized data is stored as:
   * - version (1 byte)
   * - blob_size (varint, little endian)
   * - used_hashes (varint, little endian)
   * - hashes stored in order of their type values e.g. from less significant
   to more significant
   * - hash (N bytes)
   ...
   * - hash (M bytes)
   */
  std::pair<std::unique_ptr<uint8_t[]>, size_t> dataAndSize_;
};

} // namespace facebook::eden
