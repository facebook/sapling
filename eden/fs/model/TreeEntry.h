/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "eden/fs/model/Hash.h"
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
 * can be tracked by version control.  If not, returns folly::none.
 */
std::optional<TreeEntryType> treeEntryTypeFromMode(mode_t mode);

class TreeEntry {
 public:
  explicit TreeEntry(
      const Hash& hash,
      folly::StringPiece name,
      TreeEntryType type)
      : type_(type), hash_(hash), name_(PathComponentPiece(name)) {}

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

  std::string toLogString() const;

 private:
  TreeEntryType type_;
  Hash hash_;
  PathComponent name_;
};

std::ostream& operator<<(std::ostream& os, TreeEntryType type);
bool operator==(const TreeEntry& entry1, const TreeEntry& entry2);
bool operator!=(const TreeEntry& entry1, const TreeEntry& entry2);
} // namespace eden
} // namespace facebook
