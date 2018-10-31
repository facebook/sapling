/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Range.h>
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook {
namespace eden {

class SerializedBlobMetadata {
 public:
  explicit SerializedBlobMetadata(const BlobMetadata& metadata);
  SerializedBlobMetadata(const Hash& contentsHash, uint64_t blobSize);
  folly::ByteRange slice() const;

  static BlobMetadata parse(Hash blobID, const StoreResult& result);

 private:
  void serialize(const Hash& contentsHash, uint64_t blobSize);

  static constexpr size_t SIZE = sizeof(uint64_t) + Hash::RAW_SIZE;

  /**
   * The serialized data is stored as stored as:
   * - size (8 bytes, big endian)
   * - hash (20 bytes)
   */
  std::array<uint8_t, SIZE> data_;
};
} // namespace eden
} // namespace facebook
