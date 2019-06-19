/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <algorithm>
#include <vector>
#include "Hash.h"
#include "TreeEntry.h"

namespace facebook {
namespace eden {

class Tree {
 public:
  explicit Tree(std::vector<TreeEntry>&& entries, const Hash& hash = Hash())
      : hash_(hash), entries_(std::move(entries)) {}

  const Hash& getHash() const {
    return hash_;
  }

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

 private:
  const Hash hash_;
  const std::vector<TreeEntry> entries_;
};

bool operator==(const Tree& tree1, const Tree& tree2);
bool operator!=(const Tree& tree1, const Tree& tree2);
} // namespace eden
} // namespace facebook
