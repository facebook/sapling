/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/Range.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook {
namespace eden {

class TreeMetadata;

class SerializedBlobMetadata {
 public:
  explicit SerializedBlobMetadata(const BlobMetadata& metadata);
  SerializedBlobMetadata(const Hash& contentsHash, uint64_t blobSize);
  folly::ByteRange slice() const;

  static BlobMetadata parse(Hash blobID, const StoreResult& result);

  static constexpr size_t SIZE = sizeof(uint64_t) + Hash::RAW_SIZE;

 private:
  void serialize(const Hash& contentsHash, uint64_t blobSize);
  static BlobMetadata unslice(folly::ByteRange bytes);

  /**
   * The serialized data is stored as stored as:
   * - size (8 bytes, big endian)
   * - hash (20 bytes)
   */
  std::array<uint8_t, SIZE> data_;

  friend class TreeMetadata;
};
} // namespace eden
} // namespace facebook
