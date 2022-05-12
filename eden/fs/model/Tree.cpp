/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "Tree.h"
#include <folly/io/IOBuf.h>

namespace facebook::eden {
using namespace folly;
using namespace folly::io;

bool operator==(const Tree& tree1, const Tree& tree2) {
  return (tree1.getHash() == tree2.getHash()) &&
      (tree1.entries_ == tree2.entries_);
}

bool operator!=(const Tree& tree1, const Tree& tree2) {
  return !(tree1 == tree2);
}

size_t Tree::getSizeBytes() const {
  // TODO: we should consider using a standard memory framework across
  // eden for this type of thing. D17174143 is one such idea.
  size_t internal_size = sizeof(*this);

  size_t indirect_size =
      folly::goodMallocSize(sizeof(TreeEntry) * entries_.capacity());

  for (auto& entry : entries_) {
    indirect_size += estimateIndirectMemoryUsage(entry.first.value());
  }
  return internal_size + indirect_size;
}

Tree::const_iterator Tree::find(PathComponentPiece name) const {
  auto iter = std::lower_bound(
      cbegin(),
      cend(),
      name,
      [](const Tree::value_type& entry, PathComponentPiece piece) {
        return entry.first < piece;
      });
  if (UNLIKELY(iter == cend() || iter->first != name)) {
#ifdef _WIN32
    // On a case insensitive mount, we need to do a case insensitive lookup
    // for the file and directory names. For performance, we will do a case
    // sensitive search first which should cover most of the cases and if not
    // found then do a case sensitive search.
    const auto& fileName = name.stringPiece();
    for (iter = cbegin(); iter != cend(); ++iter) {
      if (iter->first.stringPiece().equals(
              fileName, folly::AsciiCaseInsensitive())) {
        return iter;
      }
    }
#endif
    return cend();
  }
  return iter;
}

IOBuf Tree::serialize() const {
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

std::unique_ptr<Tree> Tree::tryDeserialize(
    ObjectId hash,
    folly::StringPiece data) {
  if (data.size() < sizeof(uint32_t)) {
    XLOG(ERR) << "Can not read tree version, bytes remaining " << data.size();
    return nullptr;
  }
  uint32_t version;
  memcpy(&version, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));
  if (version != V1_VERSION) {
    return nullptr;
  }

  if (data.size() < sizeof(uint32_t)) {
    XLOG(ERR) << "Can not read tree size, bytes remaining " << data.size();
    return nullptr;
  }
  uint32_t num_entries;
  memcpy(&num_entries, data.data(), sizeof(uint32_t));
  data.advance(sizeof(uint32_t));

  Tree::container entries;
  entries.reserve(num_entries);
  for (size_t i = 0; i < num_entries; i++) {
    auto entry = TreeEntry::deserialize(data);
    if (!entry) {
      return nullptr;
    }
    entries.push_back(std::move(*entry));
  }

  if (data.size() != 0u) {
    XLOG(ERR) << "Corrupted tree data, extra bytes remaining " << data.size();
    return nullptr;
  }

  return std::make_unique<Tree>(std::move(entries), std::move(hash));
}

} // namespace facebook::eden
