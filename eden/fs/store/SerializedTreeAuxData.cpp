/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/SerializedTreeAuxData.h"

#include <optional>

#include <folly/Range.h>
#include <folly/Varint.h>
#include <folly/logging/xlog.h>
#include <cstddef>

#include "eden/common/utils/Hash.h"
#include "eden/common/utils/Throw.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeAuxData.h"

namespace facebook::eden {

constexpr uint8_t kCurrentVersion = 1;

SerializedTreeAuxData::SerializedTreeAuxData(const TreeAuxData& auxData) {
  serialize(auxData.digestHash, auxData.digestSize);
}

SerializedTreeAuxData::SerializedTreeAuxData(
    const std::optional<Hash32>& digestHash,
    uint64_t digestSize) {
  serialize(digestHash, digestSize);
}

folly::ByteRange SerializedTreeAuxData::slice() const {
  return folly::ByteRange{dataAndSize_.first.get(), dataAndSize_.second};
}

/**
 * Extracts a Hash32 value from the given byte range if the BLAKE3 hash type is
 * used. For current version(v1), the BLAKE3 hash is the only hash.
 *
 * @param id The ObjectId associated with the data.
 * @param usedHashes A bitmask indicating which hash types are used.
 * @param bytes A reference to the byte range from which to extract the hash.
 * @return An optional Hash32 value if the BLAKE3 hash type is present,
 * otherwise an empty optional.
 */
std::optional<Hash32>
unsliceV1(const ObjectId& id, uint8_t usedHashes, folly::ByteRange& bytes) {
  std::optional<Hash32> blake3;
  if ((usedHashes & static_cast<uint8_t>(HashType::BLAKE3)) != 0) {
    blake3.emplace();
    readAuxDataHash(id, bytes, *blake3);
  }
  return blake3;
}

TreeAuxDataPtr unslice(const ObjectId& id, folly::ByteRange bytes) {
  // min required size is 3
  // version + size + used_hashes
  if (bytes.size() < 3 * sizeof(uint8_t)) {
    throwf<std::invalid_argument>(
        "Tree auxData for {} had unexpected size {}. Could not deserialize.",
        id,
        bytes.size());
  }

  // read version
  uint8_t version;
  memcpy(&version, bytes.data(), sizeof(uint8_t));
  bytes.advance(sizeof(uint8_t));

  if (version > kCurrentVersion || version == 0) {
    throwf<std::invalid_argument>(
        "Tree auxData for {} had unsupported version {}, expected version should be <= to {}. Could not deserialize.",
        id,
        version,
        kCurrentVersion);
  }

  const auto TreeDigestSizeExpected = folly::tryDecodeVarint(bytes);
  if (TreeDigestSizeExpected.hasError()) {
    throwf<std::invalid_argument>(
        "Failed to decode tree digest size for {}. Error: {}",
        id,
        TreeDigestSizeExpected.error() == folly::DecodeVarintError::TooFewBytes
            ? "Too few bytes"
            : "Too many bytes");
  }
  const uint64_t treeDigestSize = TreeDigestSizeExpected.value();

  const auto usedHashesExpected = folly::tryDecodeVarint(bytes);
  if (usedHashesExpected.hasError()) {
    throwf<std::invalid_argument>(
        "Failed to decode used hashes for {}. Error: {}",
        id,
        usedHashesExpected.error() == folly::DecodeVarintError::TooFewBytes
            ? "Too few bytes"
            : "Too many bytes");
  }

  switch (version) {
    case kCurrentVersion: {
      auto maybeBlake3 = unsliceV1(id, usedHashesExpected.value(), bytes);
      return std::make_shared<TreeAuxDataPtr::element_type>(
          std::move(maybeBlake3), treeDigestSize);
    }
    default:
      // dead code
      XLOGF(FATAL, "Unreachable version: {}", version);
  }

  XCHECK(bytes.empty()) << fmt::format(
      "Not all bytes were used ({} bytes left) for deserialization. Corrupted data?",
      bytes.size());
}

TreeAuxDataPtr SerializedTreeAuxData::parse(
    const ObjectId& id,
    const StoreResult& result) {
  auto bytes = result.bytes();

  return unslice(id, bytes);
}

void SerializedTreeAuxData::serialize(
    const std::optional<Hash32>& digestHash,
    uint64_t digestSize) {
  const uint8_t usedHashes = static_cast<uint8_t>(
      digestHash ? static_cast<uint8_t>(HashType::BLAKE3) : 0);

  const size_t size = sizeof(uint8_t) + folly::encodeVarintSize(digestSize) +
      folly::encodeVarintSize(usedHashes) + (digestHash ? Hash32::RAW_SIZE : 0);

  auto data = std::make_unique<uint8_t[]>(size);
  size_t off = 0;

  // version
  write(&kCurrentVersion, sizeof(uint8_t), data.get(), off);

  // tree_digest_size
  off += folly::encodeVarint(digestSize, data.get() + off);

  // used_hashes
  off += folly::encodeVarint(usedHashes, data.get() + off);

  // blake3
  if (digestHash) {
    const auto blake3Bytes = digestHash->getBytes();
    write(blake3Bytes.data(), Hash32::RAW_SIZE, data.get(), off);
  }

  XCHECK(size == off) << fmt::format(
      "Serialized data mismatch: allocated {} bytes, written {} bytes",
      size,
      off);
  dataAndSize_ = {std::move(data), size};
}

} // namespace facebook::eden
