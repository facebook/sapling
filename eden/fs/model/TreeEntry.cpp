/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/TreeEntry.h"

#include <sys/stat.h>
#include <cstdint>
#include <ostream>

#include <folly/Range.h>
#include <folly/logging/xlog.h>

#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/TimeUtil.h"

namespace facebook::eden {

using namespace folly;
using namespace folly::io;

template <typename T>
bool checkValueEqual(
    const std::optional<folly::Try<T>>& lhs,
    const std::optional<folly::Try<T>>& rhs) {
  if (!lhs.has_value() || !rhs.has_value()) {
    return lhs.has_value() == rhs.has_value();
  }
  if (lhs.value().hasException() || rhs.value().hasException()) {
    return lhs.value().hasException() == rhs.value().hasException();
  }
  return lhs.value().value() == rhs.value().value();
}

bool operator==(const EntryAttributes& lhs, const EntryAttributes& rhs) {
  return checkValueEqual(lhs.sha1, rhs.sha1) &&
      checkValueEqual(lhs.size, rhs.size) &&
      checkValueEqual(lhs.type, rhs.type) &&
      checkValueEqual(lhs.objectId, rhs.objectId) &&
      checkValueEqual(lhs.digestSize, rhs.digestSize) &&
      checkValueEqual(lhs.digestHash, rhs.digestHash) &&
      checkValueEqual(lhs.mtime, rhs.mtime) &&
      checkValueEqual(lhs.mode, rhs.mode);
}

bool operator!=(const EntryAttributes& lhs, const EntryAttributes& rhs) {
  return !(lhs == rhs);
}

bool operator==(
    const folly::Try<EntryAttributes>& lhs,
    const folly::Try<EntryAttributes>& rhs) {
  if (lhs.hasException()) {
    return rhs.hasException();
  }
  if (rhs.hasException()) {
    return lhs.hasException();
  }
  return rhs.value() == lhs.value();
}

mode_t modeFromTreeEntryType(TreeEntryType ft) {
  switch (ft) {
    case TreeEntryType::TREE:
      return S_IFDIR | 0755;
    case TreeEntryType::REGULAR_FILE:
      return S_IFREG | 0644;
    case TreeEntryType::EXECUTABLE_FILE:
      return S_IFREG | 0755;
    case TreeEntryType::SYMLINK:
      return S_IFLNK | 0755;
  }
  XLOGF(FATAL, "illegal file type {}", enumValue(ft));
}

TreeEntryType filteredEntryType(TreeEntryType ft, bool windowsSymlinksEnabled) {
  if (folly::kIsWindows) {
    if (ft != TreeEntryType::SYMLINK) {
      return ft;
    }
    return windowsSymlinksEnabled ? ft : TreeEntryType::REGULAR_FILE;
  }
  return ft;
}

dtype_t filteredEntryDtype(dtype_t mode, bool windowsSymlinksEnabled) {
  if (folly::kIsWindows) {
    if (mode != dtype_t::Symlink) {
      return mode;
    }
    return windowsSymlinksEnabled ? mode : dtype_t::Regular;
  }
  return mode;
}

std::optional<TreeEntryType> treeEntryTypeFromMode(mode_t mode) {
  if (S_ISREG(mode)) {
#ifdef _WIN32
    // On Windows, S_ISREG only means regular file and doesn't support
    // TreeEntryType::EXECUTABLE_FILE
    return TreeEntryType::REGULAR_FILE;
#else
    return mode & S_IXUSR ? TreeEntryType::EXECUTABLE_FILE
                          : TreeEntryType::REGULAR_FILE;
#endif
  } else if (S_ISLNK(mode)) {
    return TreeEntryType::SYMLINK;
  } else if (S_ISDIR(mode)) {
    return TreeEntryType::TREE;
  } else {
    return std::nullopt;
  }
}

std::string TreeEntry::toLogString(PathComponentPiece name) const {
  char fileTypeChar = '?';
  switch (type_) {
    case TreeEntryType::TREE:
      fileTypeChar = 'd';
      break;
    case TreeEntryType::REGULAR_FILE:
      fileTypeChar = 'f';
      break;
    case TreeEntryType::EXECUTABLE_FILE:
      fileTypeChar = 'x';
      break;
    case TreeEntryType::SYMLINK:
      fileTypeChar = 'l';
      break;
  }

  return fmt::format("({}, {}, {})", name, id_, fileTypeChar);
}

size_t TreeEntry::serializedSize(PathComponentPiece name) const {
  return sizeof(uint8_t) + sizeof(uint16_t) + id_.size() + sizeof(uint16_t) +
      name.view().size() + sizeof(uint64_t) + Hash20::RAW_SIZE +
      sizeof(uint8_t) + Hash32::RAW_SIZE;
}

void TreeEntry::serialize(PathComponentPiece name, Appender& appender) const {
  appender.write<uint8_t>(static_cast<uint8_t>(type_));
  auto id = id_.getBytes();
  XCHECK_LE(id.size(), std::numeric_limits<uint16_t>::max());
  appender.write<uint16_t>(folly::to_narrow(id.size()));
  appender.push(id);
  auto nameStringPiece = name.view();
  XCHECK_LE(nameStringPiece.size(), std::numeric_limits<uint16_t>::max());
  appender.write<uint16_t>(folly::to_narrow(nameStringPiece.size()));
  appender.push(folly::StringPiece{nameStringPiece});
  if (size_) {
    appender.write<uint64_t>(*size_);
  } else {
    appender.write<uint64_t>(NO_SIZE);
  }
  if (contentSha1_) {
    appender.push(contentSha1_->getBytes());
  } else {
    appender.push(kZeroHash.getBytes());
  }

  // we need to be backward compatible with the old serialization format
  // so adding a byte (with flipped bits) to distinguish between a possible
  // blake3 hash and the next entry type because we have access to the entire
  // serialized tree
  appender.write(0xff, sizeof(uint8_t));
  appender.push(contentBlake3_.value_or(kZeroHash32).getBytes());
}

std::optional<std::pair<PathComponent, TreeEntry>> TreeEntry::deserialize(
    folly::StringPiece& data) {
  uint8_t type;
  if (data.size() < sizeof(uint8_t)) {
    XLOGF(ERR, "Can not read tree entry type, bytes remaining {}", data.size());
    return std::nullopt;
  }
  memcpy(&type, data.data(), sizeof(uint8_t));
  data.advance(sizeof(uint8_t));

  uint16_t id_size;
  if (data.size() < sizeof(uint16_t)) {
    XLOGF(
        ERR,
        "Can not read tree entry id size, bytes remaining {}",
        data.size());
    return std::nullopt;
  }
  memcpy(&id_size, data.data(), sizeof(uint16_t));
  data.advance(sizeof(uint16_t));

  if (data.size() < id_size) {
    XLOGF(
        ERR,
        "Can not read tree entry id, bytes remaining {} need {}",
        data.size(),
        id_size);
    return std::nullopt;
  }
  auto id_bytes = ByteRange{StringPiece{data, 0, id_size}};
  auto id = ObjectId{id_bytes};
  data.advance(id_size);

  uint16_t name_size;
  if (data.size() < sizeof(uint16_t)) {
    XLOGF(
        ERR,
        "Can not read tree entry name size, bytes remaining {}",
        data.size());
    return std::nullopt;
  }
  memcpy(&name_size, data.data(), sizeof(uint16_t));
  data.advance(sizeof(uint16_t));

  if (data.size() < name_size) {
    XLOGF(
        ERR,
        "Can not read tree entry name, bytes remaining {} need {}",
        data.size(),
        name_size);
    return std::nullopt;
  }
  auto name_bytes = StringPiece{data, 0, name_size};
  auto name = PathComponent{name_bytes};
  data.advance(name_size);

  if (data.size() < sizeof(uint64_t)) {
    XLOGF(ERR, "Can not read tree entry size, bytes remaining {}", data.size());
    return std::nullopt;
  }
  uint64_t size_bytes;
  memcpy(&size_bytes, data.data(), sizeof(uint64_t));
  data.advance(sizeof(uint64_t));
  std::optional<uint64_t> size;
  if (size_bytes == NO_SIZE) {
    size = std::nullopt;
  } else {
    size = size_bytes;
  }

  if (data.size() < Hash20::RAW_SIZE) {
    XLOGF(ERR, "Can not read tree entry sha1, bytes remaining {}", data.size());
    return std::nullopt;
  }
  Hash20::Storage sha1_bytes;
  memcpy(&sha1_bytes, data.data(), Hash20::RAW_SIZE);
  data.advance(Hash20::RAW_SIZE);
  Hash20 sha1_raw = Hash20{sha1_bytes};
  std::optional<Hash20> sha1;
  if (sha1_raw == kZeroHash) {
    sha1 = std::nullopt;
  } else {
    sha1 = sha1_raw;
  }

  std::optional<Hash32> blake3;
  if (!data.empty() && static_cast<uint8_t>(data.data()[0]) == 0xff) {
    data.advance(1);

    if (data.size() >= Hash32::RAW_SIZE) {
      blake3.emplace();
      auto blake3Bytes = blake3->mutableBytes();
      memcpy(blake3Bytes.data(), data.data(), Hash32::RAW_SIZE);
      data.advance(Hash32::RAW_SIZE);
      if (*blake3 == kZeroHash32) {
        blake3.reset();
      }
    }
  }

  return std::pair{
      std::move(name), TreeEntry{id, (TreeEntryType)type, size, sha1, blake3}};
}

} // namespace facebook::eden
