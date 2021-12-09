/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/io/IOBuf.h>
#include <algorithm>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"

namespace facebook::eden {

class Tree {
 public:
  explicit Tree(std::vector<TreeEntry>&& entries, const ObjectId& hash)
      : hash_(hash), entries_(std::move(entries)) {}

  const ObjectId& getHash() const {
    return hash_;
  }

  /**
   * An estimate of the memory footprint of this tree. Called by ObjectCache to
   * limit the number of cached trees in memory at a time.
   */
  size_t getSizeBytes() const;

  const std::vector<TreeEntry>& getTreeEntries() const {
    return entries_;
  }

  const TreeEntry& getEntryAt(size_t index) const {
    return entries_.at(index);
  }

  const TreeEntry* getEntryPtr(PathComponentPiece path) const {
    auto iter = std::lower_bound(
        entries_.cbegin(),
        entries_.cend(),
        path,
        [](const TreeEntry& entry, PathComponentPiece piece) {
          return entry.getName() < piece;
        });
    if (UNLIKELY(iter == entries_.cend() || iter->getName() != path)) {
#ifdef _WIN32
      // On Windows we need to do a case insensitive lookup for the file and
      // directory names. For performance, we will do a case sensitive search
      // first which should cover most of the cases and if not found then do a
      // case sensitive search.
      const auto& fileName = path.stringPiece();
      for (const auto& entry : entries_) {
        if (entry.getName().stringPiece().equals(
                fileName, folly::AsciiCaseInsensitive())) {
          return &entry;
        }
      }
#endif
      return nullptr;
    }
    return &*iter;
  }

  const TreeEntry& getEntryAt(PathComponentPiece path) const {
    auto entry = getEntryPtr(path);
    if (!entry) {
      throw std::out_of_range(
          folly::to<std::string>(path, " is not present in this Tree"));
    }
    return *entry;
  }

  std::vector<PathComponent> getEntryNames() const {
    std::vector<PathComponent> results;
    results.reserve(entries_.size());
    for (const auto& entry : entries_) {
      results.emplace_back(entry.getName());
    }
    return results;
  }

  bool isGitTreeCompatible() const {
    for (const auto& entry : entries_) {
      if (entry.getHash().size() != Hash20::RAW_SIZE) {
        return false;
      }
    }
    return true;
  }

  /**
   * Serialize tree using custom format.
   */
  folly::IOBuf serialize() const;

  /**
   * Deserialize tree if possible.
   * Returns nullopt if serialization format is not supported.
   *
   * First byte is used to identify serialization format.
   * Git tree starts with 'tree', so we can use any bytes other then 't' as a
   * version identifier. Currently only V1_VERSION is supported, along with git
   * tree format.
   */
  static std::optional<Tree> tryDeserialize(
      ObjectId hash,
      folly::StringPiece data);

 private:
  const ObjectId hash_;
  const std::vector<TreeEntry> entries_;

  static constexpr uint32_t V1_VERSION = 1u;
};

bool operator==(const Tree& tree1, const Tree& tree2);
bool operator!=(const Tree& tree1, const Tree& tree2);

} // namespace facebook::eden
