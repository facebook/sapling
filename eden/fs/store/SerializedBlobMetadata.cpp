/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "SerializedBlobMetadata.h"
#include <folly/Format.h>
namespace facebook {
namespace eden {

SerializedBlobMetadata::SerializedBlobMetadata(const BlobMetadata& metadata) {
  serialize(metadata.sha1, metadata.size);
}

SerializedBlobMetadata::SerializedBlobMetadata(
    const Hash& contentsHash,
    uint64_t blobSize) {
  serialize(contentsHash, blobSize);
}

folly::ByteRange SerializedBlobMetadata::slice() const {
  return folly::ByteRange{data_};
}

BlobMetadata SerializedBlobMetadata::parse(
    Hash blobID,
    const StoreResult& result) {
  auto bytes = result.bytes();
  if (bytes.size() != SIZE) {
    throw std::invalid_argument(folly::sformat(
        "Blob metadata for {} had unexpected size {}. Could not deserialize.",
        blobID.toString(),
        bytes.size()));
  }

  uint64_t blobSizeBE;
  memcpy(&blobSizeBE, bytes.data(), sizeof(uint64_t));
  bytes.advance(sizeof(uint64_t));
  auto contentsHash = Hash{bytes};
  return BlobMetadata{contentsHash, folly::Endian::big(blobSizeBE)};
}

void SerializedBlobMetadata::serialize(
    const Hash& contentsHash,
    uint64_t blobSize) {
  uint64_t blobSizeBE = folly::Endian::big(blobSize);
  memcpy(data_.data(), &blobSizeBE, sizeof(uint64_t));
  memcpy(
      data_.data() + sizeof(uint64_t),
      contentsHash.getBytes().data(),
      Hash::RAW_SIZE);
}

} // namespace eden
} // namespace facebook
