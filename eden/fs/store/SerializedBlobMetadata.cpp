/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/SerializedBlobMetadata.h"

#include <optional>

#include <folly/Range.h>
#include <folly/Varint.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>
#include <cstddef>

#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/Throw.h"

namespace facebook::eden {

SerializedBlobMetadata::SerializedBlobMetadata(const BlobMetadata& metadata) {
  serialize(metadata.sha1, metadata.blake3, metadata.size);
}

SerializedBlobMetadata::SerializedBlobMetadata(
    const Hash20& sha1,
    const std::optional<Hash32>& blake3,
    uint64_t blobSize) {
  serialize(sha1, blake3, blobSize);
}

folly::ByteRange SerializedBlobMetadata::slice() const {
  return folly::ByteRange{dataAndSize_.first.get(), dataAndSize_.second};
}

namespace {
// bit enum representing possible hash types that could be used
// 8 should be more than enough for now
// but still this enum is represented as a varint
enum class HashType : uint8_t {
  SHA1 = (1 << 0),
  BLAKE3 = (1 << 1),
};

constexpr size_t kLegacySize = sizeof(uint64_t) + Hash20::RAW_SIZE;
constexpr uint8_t kCurrentVersion = 1;

FOLLY_ALWAYS_INLINE void
write(const uint8_t* src, size_t len, uint8_t* dest, size_t& off) {
  memcpy(dest + off, src, len);
  off += len;
}

BlobMetadataPtr unsliceLegacy(folly::ByteRange bytes) {
  uint64_t blobSizeBE;
  memcpy(&blobSizeBE, bytes.data(), sizeof(uint64_t));
  bytes.advance(sizeof(uint64_t));
  auto contentsHash = Hash20{bytes};
  return std::make_shared<BlobMetadataPtr::element_type>(
      contentsHash, std::nullopt, folly::Endian::big(blobSizeBE));
}

template <size_t SIZE>
void readHash(
    const ObjectId& blobID,
    folly::ByteRange& bytes,
    Hash<SIZE>& hash) {
  if (bytes.size() < SIZE) {
    throwf<std::invalid_argument>(
        "Blob metadata for {} had unexpected size {}. Could not deserialize the hash of size {}.",
        blobID,
        bytes.size(),
        SIZE);
  }

  auto mutableBytes = hash.mutableBytes();
  memcpy(mutableBytes.data(), bytes.data(), mutableBytes.size());
  bytes.advance(mutableBytes.size());
}

std::pair<Hash20, std::optional<Hash32>>
unsliceV1(const ObjectId& blobID, uint8_t usedHashes, folly::ByteRange& bytes) {
  if ((usedHashes & static_cast<uint8_t>(HashType::SHA1)) == 0) {
    throwf<std::invalid_argument>(
        "Blob metadata for {} doesn't have SHA1 hash which is mandatory. Could not deserialize.",
        blobID);
  }

  Hash20 sha1;
  readHash(blobID, bytes, sha1);

  std::optional<Hash32> blake3;
  if ((usedHashes & static_cast<uint8_t>(HashType::BLAKE3)) != 0) {
    blake3.emplace();
    readHash(blobID, bytes, *blake3);
  }

  return {std::move(sha1), std::move(blake3)};
}

BlobMetadataPtr unslice(const ObjectId& blobID, folly::ByteRange bytes) {
  // min required size is 3
  // version + size + used_hashes
  if (bytes.size() < 3 * sizeof(uint8_t)) {
    throwf<std::invalid_argument>(
        "Blob metadata for {} had unexpected size {}. Could not deserialize.",
        blobID,
        bytes.size());
  }

  // read version
  uint8_t version;
  memcpy(&version, bytes.data(), sizeof(uint8_t));
  bytes.advance(sizeof(uint8_t));

  if (version > kCurrentVersion || version == 0) {
    throwf<std::invalid_argument>(
        "Blob metadata for {} had unsupported version {}, expected version should be <= to {}. Could not deserialize.",
        blobID,
        version,
        kCurrentVersion);
  }

  const auto blobSizeExpected = folly::tryDecodeVarint(bytes);
  if (blobSizeExpected.hasError()) {
    throwf<std::invalid_argument>(
        "Failed to decode blob size for {}. Error: {}",
        blobID,
        blobSizeExpected.error() == folly::DecodeVarintError::TooFewBytes
            ? "Too few bytes"
            : "Too many bytes");
  }
  const uint64_t blobSize = blobSizeExpected.value();

  const auto usedHashesExpected = folly::tryDecodeVarint(bytes);
  if (usedHashesExpected.hasError()) {
    throwf<std::invalid_argument>(
        "Failed to decode used hashes for {}. Error: {}",
        blobID,
        usedHashesExpected.error() == folly::DecodeVarintError::TooFewBytes
            ? "Too few bytes"
            : "Too many bytes");
  }

  switch (version) {
    case kCurrentVersion: {
      auto [sha1, maybeBlake3] =
          unsliceV1(blobID, usedHashesExpected.value(), bytes);
      return std::make_shared<BlobMetadataPtr::element_type>(
          std::move(sha1), std::move(maybeBlake3), blobSize);
    }
    default:
      // dead code
      XLOGF(FATAL, "Unreachable version: {}", version);
  }

  XCHECK(bytes.empty()) << fmt::format(
      "Not all bytes were used ({} bytes left) for deserialization. Corrupted data?",
      bytes.size());
}
} // namespace

BlobMetadataPtr SerializedBlobMetadata::parse(
    const ObjectId& blobID,
    const StoreResult& result) {
  auto bytes = result.bytes();
  // check if we deal with legacy format
  // size is 28 and the first byte is 0 (we store the size in big endian and
  // unlikely that someone stored such a big blob with size of 2^64)
  if (bytes.size() == kLegacySize && bytes[0] == 0) {
    return unsliceLegacy(bytes);
  }

  return unslice(blobID, bytes);
}

void SerializedBlobMetadata::serialize(
    const Hash20& sha1,
    const std::optional<Hash32>& blake3,
    uint64_t blobSize) {
  const uint8_t usedHashes = static_cast<uint8_t>(HashType::SHA1) |
      static_cast<uint8_t>(blake3 ? static_cast<uint8_t>(HashType::BLAKE3) : 0);
  const size_t size = sizeof(uint8_t) + folly::encodeVarintSize(blobSize) +
      folly::encodeVarintSize(usedHashes) + Hash20::RAW_SIZE +
      (blake3 ? Hash32::RAW_SIZE : 0);
  auto data = std::make_unique<uint8_t[]>(size);
  size_t off = 0;

  // version
  write(&kCurrentVersion, sizeof(uint8_t), data.get(), off);

  // blob_size
  off += folly::encodeVarint(blobSize, data.get() + off);

  // used_hashes
  off += folly::encodeVarint(usedHashes, data.get() + off);

  // sha1
  const auto sha1Bytes = sha1.getBytes();
  write(sha1Bytes.data(), Hash20::RAW_SIZE, data.get(), off);

  // blake3
  if (blake3) {
    const auto blake3Bytes = blake3->getBytes();
    write(blake3Bytes.data(), Hash32::RAW_SIZE, data.get(), off);
  }

  XCHECK(size == off) << fmt::format(
      "Serialized data mismatch: allocated {} bytes, written {} bytes",
      size,
      off);
  dataAndSize_ = {std::move(data), size};
}

} // namespace facebook::eden
