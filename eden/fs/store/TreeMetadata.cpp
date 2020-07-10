/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/TreeMetadata.h"

#include <folly/Conv.h>
#include <folly/io/Cursor.h>
#include <folly/lang/Bits.h>

#include "eden/fs/store/BlobMetadata.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook {
namespace eden {

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

size_t TreeMetadata::getSerializedSize() const {
  return sizeof(uint32_t) + ENTRY_SIZE * getNumberOfEntries();
}

IOBuf TreeMetadata::serialize() const {
  // serialize tree metadata as: <number of entries><hash for first
  // entry><serialized metadata for first entry> ... <hash for last
  // entry><serialized metadata for last entry>
  IOBuf buf(IOBuf::CREATE, getSerializedSize());
  Appender appender(&buf, 0);

  auto numberOfEntries = getNumberOfEntries();
  CHECK_LE(numberOfEntries, std::numeric_limits<uint32_t>::max());
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

TreeMetadata TreeMetadata::deserialize(const StoreResult& result) {
  auto data = result.piece();
  if (data.size() < sizeof(uint32_t)) {
    throw std::invalid_argument(
        "buffer too small -- serialized tree contains unknown number of entries");
  }
  uint32_t numberOfEntries;
  memcpy(&numberOfEntries, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));

  if (data.size() < numberOfEntries * ENTRY_SIZE) {
    throw std::invalid_argument(
        "buffer too small -- serialized tree does not contain metadata for all "
        " entries");
  }

  HashIndexedEntryMetadata entryMetadata;
  entryMetadata.reserve(numberOfEntries);
  for (uint32_t i = 0; i < numberOfEntries; ++i) {
    auto temp = ByteRange{StringPiece{data, 0, Hash::RAW_SIZE}};
    auto hash = Hash{temp};
    data.advance(Hash::RAW_SIZE);

    auto serializedMetadata =
        ByteRange{StringPiece{data, 0, SerializedBlobMetadata::SIZE}};
    BlobMetadata metadata = SerializedBlobMetadata::unslice(serializedMetadata);
    data.advance(SerializedBlobMetadata::SIZE);

    entryMetadata.push_back(std::make_pair(hash, metadata));
  }

  return TreeMetadata(entryMetadata);
}
} // namespace eden
} // namespace facebook
