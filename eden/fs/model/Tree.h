/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
  explicit Tree(const Hash& hash, std::vector<TreeEntry>&& entries)
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

 private:
  const Hash hash_;
  const std::vector<TreeEntry> entries_;
};
}
}
