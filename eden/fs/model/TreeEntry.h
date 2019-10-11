/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/model/Hash.h"
#ifndef _WIN32
#include "eden/fs/utils/DirType.h"
#endif
#include "eden/fs/utils/PathFuncs.h"

#include <folly/String.h>
#include <iosfwd>
#include <optional>

namespace facebook {
namespace eden {

class Hash;

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
      const Hash& hash,
      folly::StringPiece name,
      TreeEntryType type)
      : type_(type), hash_(hash), name_(PathComponentPiece(name)) {}

  explicit TreeEntry(
      const Hash& hash,
      folly::StringPiece name,
      TreeEntryType type,
      std::optional<uint64_t> size,
      std::optional<Hash> contentSha1)
      : type_(type),
        hash_(hash),
        name_(PathComponentPiece(name)),
        size_(size),
        contentSha1_(contentSha1) {}

  const Hash& getHash() const {
    return hash_;
  }

  const PathComponent& getName() const {
    return name_;
  }

  bool isTree() const {
    return type_ == TreeEntryType::TREE;
  }

  TreeEntryType getType() const {
    return type_;
  }

#ifndef _WIN32
  dtype_t getDType() const {
    switch (type_) {
      case TreeEntryType::TREE:
        return dtype_t::Dir;
      case TreeEntryType::REGULAR_FILE:
      case TreeEntryType::EXECUTABLE_FILE:
        return dtype_t::Regular;
      case TreeEntryType::SYMLINK:
        return dtype_t::Symlink;
      default:
        return dtype_t::Unknown;
    }
  }
#endif //! _WIN32

  std::string toLogString() const;

  const std::optional<uint64_t>& getSize() const {
    return size_;
  }

  const std::optional<Hash>& getContentSha1() const {
    return contentSha1_;
  }

 private:
  TreeEntryType type_;
  Hash hash_;
  PathComponent name_;
  std::optional<uint64_t> size_;
  std::optional<Hash> contentSha1_;
};

std::ostream& operator<<(std::ostream& os, TreeEntryType type);
bool operator==(const TreeEntry& entry1, const TreeEntry& entry2);
bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2);
} // namespace eden
} // namespace facebook
