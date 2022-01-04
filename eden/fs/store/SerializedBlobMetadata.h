/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/Range.h>
#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook::eden {

class TreeMetadata;

class SerializedBlobMetadata {
 public:
  explicit SerializedBlobMetadata(const BlobMetadata& metadata);
  SerializedBlobMetadata(const Hash20& contentsHash, uint64_t blobSize);
  folly::ByteRange slice() const;

  static BlobMetadata parse(ObjectId blobID, const StoreResult& result);

  static constexpr size_t SIZE = sizeof(uint64_t) + Hash20::RAW_SIZE;

 private:
  void serialize(const Hash20& contentsHash, uint64_t blobSize);
  static BlobMetadata unslice(folly::ByteRange bytes);

  /**
   * The serialized data is stored as stored as:
   * - size (8 bytes, big endian)
   * - hash (20 bytes)
   */
  std::array<uint8_t, SIZE> data_;

  friend class TreeMetadata;
};

} // namespace facebook::eden
