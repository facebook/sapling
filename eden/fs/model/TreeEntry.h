/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

#include <folly/String.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <iosfwd>
#include <optional>

namespace facebook::eden {

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
  explicit TreeEntry(
      const ObjectId& hash,
      PathComponent name,
      TreeEntryType type)
      : type_(type), hash_(hash), name_(std::move(name)) {}

  explicit TreeEntry(
      const ObjectId& hash,
      PathComponent name,
      TreeEntryType type,
      std::optional<uint64_t> size,
      std::optional<Hash20> contentSha1)
      : type_(type),
        hash_(hash),
        name_(std::move(name)),
        size_(size),
        contentSha1_(contentSha1) {}

  const ObjectId& getHash() const {
    return hash_;
  }

  const PathComponent& getName() const {
    return name_;
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

  std::string toLogString() const;

  const std::optional<uint64_t>& getSize() const {
    return size_;
  }

  const std::optional<Hash20>& getContentSha1() const {
    return contentSha1_;
  }

  /**
   * An estimate of the memory footprint of this treeEntry outside of the data
   * directly stored in this object.
   */
  size_t getIndirectSizeBytes() const;

  /**
   * Computes exact serialized size of this entry.
   */
  size_t serializedSize() const;

  /**
   * Serializes entry into appender, consuming exactly serializedSize() bytes.
   */
  void serialize(folly::io::Appender& appender) const;

  /**
   * Deserialize tree entry.
   */
  static std::optional<TreeEntry> deserialize(folly::StringPiece& data);

 private:
  TreeEntryType type_;
  ObjectId hash_;
  PathComponent name_;
  std::optional<uint64_t> size_;
  std::optional<Hash20> contentSha1_;

  static constexpr uint64_t NO_SIZE = std::numeric_limits<uint64_t>::max();
};

std::ostream& operator<<(std::ostream& os, TreeEntryType type);
bool operator==(const TreeEntry& entry1, const TreeEntry& entry2);
bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2);

} // namespace facebook::eden
