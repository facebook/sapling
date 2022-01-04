/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeMetadata.h"

#include <folly/Conv.h>
#include <folly/io/Cursor.h>
#include <folly/lang/Bits.h>
#include <folly/logging/xlog.h>

#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook::eden {

using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;

TreeMetadata::TreeMetadata(EntryMetadata entryMetadata)
    : entryMetadata_(std::move(entryMetadata)) {}

size_t TreeMetadata::getNumberOfEntries() const {
  return std::visit(
      [](auto&& entryMetadata) { return entryMetadata.size(); },
      entryMetadata_);
}

// Serializes tree metadata into buffer.
// If tree metadata consists only of 20-bytes hashes, it serializes into V1
// format.
//
// V1 format is compatible with eden fs versions that had fixed length hashes.
// We try to serialize into V1 format if possible, to allow safe rollback
// between older EdenFS versions and this version.
//
// If this tree metadata has hashes that are not 20-bytes, serializes into V2
// format. To distinguish from V1 format, first bit of <number of entries> for
// V2 format is set to 1. V2 format includes hash length for each hash to
// support variable hash length.
//
// We assume here that existing EdenFs installations do not have directories
// with more then 2^31 entries, so that size of V1 format can not be mistaken
// for V2 format market, which seem like reasonable assumption.
IOBuf TreeMetadata::serialize() const {
  auto hashIndexedEntries =
      std::get_if<HashIndexedEntryMetadata>(&entryMetadata_);
  if (!hashIndexedEntries) {
    throw std::domain_error(
        "Identifiers for entries are not hashes, can not serialize.");
  }
  bool can_serialize_v1 = true;
  // <NumberOfEntries> <Version>
  size_t serialized_size_v2 = sizeof(uint32_t) + sizeof(uint32_t);
  for (auto& [hash, _] : *hashIndexedEntries) {
    serialized_size_v2 +=
        sizeof(uint16_t) + hash.size() + SerializedBlobMetadata::SIZE;
    if (hash.size() != Hash20::RAW_SIZE) {
      can_serialize_v1 = false;
    }
  }
  if (can_serialize_v1) {
    return serializeV1();
  }
  return serializeV2(serialized_size_v2);
}

// This requires that all ids are 20-byte hashes
// V1 format is compatible with previous EdenFS versions
// We try to serialize into V1 format if possible, to simplify revert of eden fs
// version
IOBuf TreeMetadata::serializeV1() const {
  // serialize tree metadata as: <number of entries><hash for first
  // entry><serialized metadata for first entry> ... <hash for last
  // entry><serialized metadata for last entry>
  size_t serialized_size =
      sizeof(uint32_t) + ENTRY_SIZE_V1 * getNumberOfEntries();
  IOBuf buf(IOBuf::CREATE, serialized_size);
  Appender appender(&buf, 0);

  auto numberOfEntries = getNumberOfEntries();
  XCHECK_LT(numberOfEntries, SERIALIZED_V2_MARKER);
  appender.write<uint32_t>(folly::to_narrow(numberOfEntries));
  auto hashIndexedEntries =
      std::get_if<HashIndexedEntryMetadata>(&entryMetadata_);
  if (!hashIndexedEntries) {
    throw std::domain_error(
        "Identifiers for entries are not hashes, can not serialize.");
  }
  for (auto& [hash, metadata] : *hashIndexedEntries) {
    appender.push(hash.getBytes());
    SerializedBlobMetadata serializedMetadata(metadata);
    appender.push(serializedMetadata.slice());
  }
  return buf;
}

// V2 format supports variable length hashes
IOBuf TreeMetadata::serializeV2(size_t serialized_size) const {
  // serialize tree metadata as: <number of entries><size of hash for first
  // entry><hash for first entry><serialized metadata for first entry> ... <size
  // of hash for last entry><hash for last entry><serialized metadata for last
  // entry>
  //
  // In this format highest bit in number of entries is set to 1 to distinguish
  // from V1 format
  IOBuf buf(IOBuf::CREATE, serialized_size);
  Appender appender(&buf, 0);

  auto numberOfEntries = getNumberOfEntries();
  XCHECK_LT(numberOfEntries, SERIALIZED_V2_MARKER);
  uint32_t numberOfEntries32 = folly::to_narrow(numberOfEntries);
  appender.write<uint32_t>(numberOfEntries32 | SERIALIZED_V2_MARKER);
  appender.write<uint32_t>(V2_VERSION);
  auto hashIndexedEntries =
      std::get_if<HashIndexedEntryMetadata>(&entryMetadata_);
  if (!hashIndexedEntries) {
    throw std::domain_error(
        "Identifiers for entries are not hashes, can not serialize.");
  }
  for (auto& [hash, metadata] : *hashIndexedEntries) {
    auto bytes = hash.getBytes();
    XCHECK_LE(bytes.size(), std::numeric_limits<uint16_t>::max());
    appender.write<uint16_t>(folly::to_narrow(bytes.size()));
    appender.push(bytes);
    SerializedBlobMetadata serializedMetadata(metadata);
    appender.push(serializedMetadata.slice());
  }
  return buf;
}

TreeMetadata TreeMetadata::deserialize(const StoreResult& result) {
  auto data = result.piece();
  if (data.size() < sizeof(uint32_t)) {
    throw std::invalid_argument(
        "buffer too small -- serialized tree contains unknown number of entries");
  }
  uint32_t numberOfEntries;
  memcpy(&numberOfEntries, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));
  if ((numberOfEntries & SERIALIZED_V2_MARKER) == SERIALIZED_V2_MARKER) {
    return deserializeV2(data, numberOfEntries - SERIALIZED_V2_MARKER);
  }
  return deserializeV1(data, numberOfEntries);
}

TreeMetadata TreeMetadata::deserializeV1(
    folly::StringPiece data,
    uint32_t numberOfEntries) {
  if (data.size() < numberOfEntries * ENTRY_SIZE_V1) {
    throw std::invalid_argument(
        "buffer too small -- serialized tree does not contain metadata for all "
        " entries");
  }

  HashIndexedEntryMetadata entryMetadata;
  entryMetadata.reserve(numberOfEntries);
  for (uint32_t i = 0; i < numberOfEntries; ++i) {
    auto temp = ByteRange{StringPiece{data, 0, Hash20::RAW_SIZE}};
    auto hash = ObjectId{temp};
    data.advance(Hash20::RAW_SIZE);

    auto serializedMetadata =
        ByteRange{StringPiece{data, 0, SerializedBlobMetadata::SIZE}};
    BlobMetadata metadata = SerializedBlobMetadata::unslice(serializedMetadata);
    data.advance(SerializedBlobMetadata::SIZE);

    entryMetadata.push_back(std::make_pair(hash, metadata));
  }

  return TreeMetadata(entryMetadata);
}

TreeMetadata TreeMetadata::deserializeV2(
    folly::StringPiece data,
    uint32_t numberOfEntries) {
  uint32_t version_marker;
  memcpy(&version_marker, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));
  XCHECK_EQ(version_marker, V2_VERSION);
  HashIndexedEntryMetadata entryMetadata;
  entryMetadata.reserve(numberOfEntries);
  for (uint32_t i = 0; i < numberOfEntries; ++i) {
    uint16_t size;
    memcpy(&size, data.data(), sizeof(uint16_t));
    data.advance(sizeof(uint16_t));
    auto temp = ByteRange{StringPiece{data, 0, size}};
    auto hash = ObjectId{temp};
    data.advance(size);

    auto serializedMetadata =
        ByteRange{StringPiece{data, 0, SerializedBlobMetadata::SIZE}};
    BlobMetadata metadata = SerializedBlobMetadata::unslice(serializedMetadata);
    data.advance(SerializedBlobMetadata::SIZE);

    entryMetadata.push_back(std::make_pair(hash, metadata));
  }

  return TreeMetadata(entryMetadata);
}

} // namespace facebook::eden
