/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/Tree.h"
#include <folly/io/IOBuf.h>
#include "eden/fs/model/TreeAuxData.h"

namespace facebook::eden {

using namespace folly;
using namespace folly::io;

size_t Tree::getSizeBytes() const {
  // TODO: we should consider using a standard memory framework across
  // eden for this type of thing. D17174143 is one such idea.
  size_t internal_size = sizeof(*this);

  size_t indirect_size =
      folly::goodMallocSize(sizeof(TreeEntry) * entries_.capacity());

  for (auto& entry : entries_) {
    indirect_size += estimateIndirectMemoryUsage(entry.first.value());
  }

  size_t auxDataSize = 0;
  if (auxData_) {
    auxDataSize = sizeof(uint64_t) +
        (auxData_->digestHash.has_value() ? Hash32::RAW_SIZE : 0);
  }
  return internal_size + indirect_size + auxDataSize;
}

IOBuf Tree::serialize_v1() const {
  size_t serialized_size = sizeof(uint32_t) + sizeof(uint32_t);
  for (auto& entry : entries_) {
    serialized_size += entry.second.serializedSize(entry.first);
  }
  IOBuf buf(IOBuf::CREATE, serialized_size);
  Appender appender(&buf, 0);

  XCHECK_LE(entries_.size(), std::numeric_limits<uint32_t>::max());
  uint32_t numberOfEntries = static_cast<uint32_t>(entries_.size());

  appender.write<uint32_t>(V1_VERSION);
  appender.write<uint32_t>(numberOfEntries);
  for (auto& entry : entries_) {
    entry.second.serialize(entry.first, appender);
  }

  return buf;
}

IOBuf Tree::serialize() const {
  size_t serialized_size = sizeof(uint32_t) + sizeof(uint32_t);
  for (auto& entry : entries_) {
    serialized_size += entry.second.serializedSize(entry.first);
  }

  if (auxData_) {
    // digestSize + (maybe) digestHash
    serialized_size += sizeof(uint64_t);
    if (auxData_->digestHash.has_value()) {
      serialized_size += Hash32::RAW_SIZE;
    }
  }

  IOBuf buf(IOBuf::CREATE, serialized_size);
  Appender appender(&buf, 0);

  XCHECK_LE(entries_.size(), std::numeric_limits<uint32_t>::max());
  uint32_t numberOfEntries = static_cast<uint32_t>(entries_.size());

  appender.write<uint32_t>(V2_VERSION);
  appender.write<uint32_t>(numberOfEntries);
  for (auto& entry : entries_) {
    entry.second.serialize(entry.first, appender);
  }

  if (auxData_) {
    // Serialize the digestSize so we can save a few bytes
    // if there is no digestHash.
    appender.write<uint64_t>(auxData_->digestSize);
    if (auxData_->digestHash.has_value()) {
      appender.push(auxData_->digestHash.value().getBytes());
    }
  }
  return buf;
}

TreePtr Tree::tryDeserialize(ObjectId hash, folly::StringPiece data) {
  if (data.size() < sizeof(uint32_t)) {
    XLOGF(ERR, "Can not read tree version, bytes remaining {}", data.size());
    return nullptr;
  }
  uint32_t version;
  memcpy(&version, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));
  if (version != V1_VERSION && version != V2_VERSION) {
    XLOGF(WARN, "Git tree version? {}", version);
    return nullptr;
  }

  if (data.size() < sizeof(uint32_t)) {
    XLOGF(ERR, "Can not read tree size, bytes remaining {}", data.size());
    return nullptr;
  }
  uint32_t num_entries;
  memcpy(&num_entries, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));

  Tree::container entries{kPathMapDefaultCaseSensitive};
  entries.reserve(num_entries);
  for (size_t i = 0; i < num_entries; i++) {
    auto entry = TreeEntry::deserialize(data);
    if (!entry) {
      return nullptr;
    }
    entries.emplace(entry->first, std::move(entry->second));
  }

  if (version == V1_VERSION && !data.empty()) {
    XLOGF(
        ERR,
        "Corrupted version {} tree data, extra {} bytes remaining",
        version,
        data.size());
    return nullptr;
  }

  // backwards compatibility for V1 Tree version
  if (version == V1_VERSION || data.empty()) {
    return std::make_shared<TreePtr::element_type>(std::move(entries), hash);
  }

  // V2 Tree version
  if (data.size() < (sizeof(uint64_t))) {
    XLOGF(
        ERR, "Corrupted version 2 tree data, {} bytes remaining", data.size());
    return nullptr;
  }

  // deserialize the tree aux data
  uint64_t digestSize;
  memcpy(&digestSize, data.data(), sizeof(uint64_t));
  data.advance(sizeof(uint64_t));

  std::optional<Hash32> digestHash;

  if (data.empty()) {
    digestHash = std::nullopt;
  } else {
    if (data.size() < Hash32::RAW_SIZE) {
      XLOGF(
          ERR,
          "Corrupted version 2 tree data, {} bytes remaining",
          data.size());
      return nullptr;
    }

    Hash32::Storage digest_hash_bytes;
    memcpy(&digest_hash_bytes, data.data(), Hash32::RAW_SIZE);
    data.advance(Hash32::RAW_SIZE);
    if (!data.empty()) {
      XLOGF(
          ERR,
          "Corrupted version 2 tree data, {} bytes remaining",
          data.size());
      return nullptr;
    }
    digestHash.emplace(digest_hash_bytes);
  }

  // All good for V2 Tree version, append the aux data
  return std::make_shared<TreePtr::element_type>(
      std::move(hash),
      std::move(entries),
      std::make_shared<TreeAuxDataPtr::element_type>(
          std::move(digestHash), digestSize));
}

} // namespace facebook::eden
