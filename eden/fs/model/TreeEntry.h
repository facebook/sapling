/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <iosfwd>
#include <optional>

#include <folly/String.h>
#include <folly/Try.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>

#include "eden/common/utils/OptionSet.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
class BlobMetadata;

struct EntryAttributeFlags
    : OptionSet<EntryAttributeFlags, std::underlying_type_t<FileAttributes>> {
  constexpr static EntryAttributeFlags raw(FileAttributes raw) {
    return OptionSet<
        EntryAttributeFlags,
        std::underlying_type_t<FileAttributes>>::raw(folly::to_underlying(raw));
  }
  constexpr static EntryAttributeFlags raw(
      std::underlying_type_t<FileAttributes> raw) {
    return OptionSet<
        EntryAttributeFlags,
        std::underlying_type_t<FileAttributes>>::raw(raw);
  }
};

inline constexpr auto ENTRY_ATTRIBUTE_TYPE =
    EntryAttributeFlags::raw(FileAttributes::SOURCE_CONTROL_TYPE);
inline constexpr auto ENTRY_ATTRIBUTE_SIZE =
    EntryAttributeFlags::raw(FileAttributes::FILE_SIZE);
inline constexpr auto ENTRY_ATTRIBUTE_SHA1 =
    EntryAttributeFlags::raw(FileAttributes::SHA1_HASH);

/**
 * Represents the allowed types of entries in version control trees.
 *
 * Currently missing from this list: git submodules.
 */
enum class TreeEntryType : uint8_t {
  TREE,
  REGULAR_FILE,
  EXECUTABLE_FILE,
  SYMLINK,
};

class EntryAttributes {
 public:
  EntryAttributes(
      std::optional<folly::Try<Hash20>> contentsHash,
      std::optional<folly::Try<uint64_t>> fileLength,
      std::optional<folly::Try<TreeEntryType>> fileType);

  std::optional<folly::Try<Hash20>> sha1;
  std::optional<folly::Try<uint64_t>> size;
  std::optional<folly::Try<TreeEntryType>> type;
};

/**
 * Comparing two EntryAttributes or Try of EntryAttributes, exceptions of any
 * kind are considered equal for simplicity.
 */
bool operator==(const EntryAttributes& lhs, const EntryAttributes& rhs);
bool operator!=(const EntryAttributes& lhs, const EntryAttributes& rhs);
bool operator==(
    const folly::Try<EntryAttributes>& lhs,
    const folly::Try<EntryAttributes>& rhs);

/**
 * Computes an initial mode_t, including permission bits, from a FileType.
 */
mode_t modeFromTreeEntryType(TreeEntryType ft);

/**
 * Converts an arbitrary mode_t to the appropriate TreeEntryType if the file
 * can be tracked by version control.  If not, returns std::nullopt.
 */
std::optional<TreeEntryType> treeEntryTypeFromMode(mode_t mode);

class TreeEntry {
 public:
  explicit TreeEntry(const ObjectId& hash, TreeEntryType type)
      : type_(type), hash_(std::move(hash)) {}

  explicit TreeEntry(
      const ObjectId& hash,
      TreeEntryType type,
      std::optional<uint64_t> size,
      std::optional<Hash20> contentSha1)
      : type_(type),
        hash_(std::move(hash)),
        size_(size),
        contentSha1_(contentSha1) {}

  const ObjectId& getHash() const {
    return hash_;
  }

  bool isTree() const {
    return type_ == TreeEntryType::TREE;
  }

  TreeEntryType getType() const {
#ifdef _WIN32
    // XXX(T66590035): instead of doing this here, this should be done in the
    // Windows specific code that interpret these.
    switch (type_) {
      case TreeEntryType::REGULAR_FILE:
      case TreeEntryType::EXECUTABLE_FILE:
      case TreeEntryType::SYMLINK:
        return TreeEntryType::REGULAR_FILE;
      default:
        return type_;
    }
#else
    return type_;
#endif
  }

  dtype_t getDType() const {
    switch (type_) {
      case TreeEntryType::TREE:
        return dtype_t::Dir;
      case TreeEntryType::REGULAR_FILE:
      case TreeEntryType::EXECUTABLE_FILE:
        return dtype_t::Regular;
#ifndef _WIN32
      case TreeEntryType::SYMLINK:
        return dtype_t::Symlink;
#endif
      default:
        return dtype_t::Unknown;
    }
  }

  std::string toLogString(PathComponentPiece name) const;

  const std::optional<uint64_t>& getSize() const {
    return size_;
  }

  const std::optional<Hash20>& getContentSha1() const {
    return contentSha1_;
  }

  /**
   * Computes exact serialized size of this entry.
   */
  size_t serializedSize(PathComponentPiece name) const;

  /**
   * Serializes entry into appender, consuming exactly serializedSize() bytes.
   */
  void serialize(PathComponentPiece name, folly::io::Appender& appender) const;

  /**
   * Deserialize tree entry.
   */
  static std::optional<std::pair<PathComponent, TreeEntry>> deserialize(
      folly::StringPiece& data);

 private:
  TreeEntryType type_;
  ObjectId hash_;
  std::optional<uint64_t> size_;
  std::optional<Hash20> contentSha1_;

  static constexpr uint64_t NO_SIZE = std::numeric_limits<uint64_t>::max();
};

std::ostream& operator<<(std::ostream& os, TreeEntryType type);

} // namespace facebook::eden
