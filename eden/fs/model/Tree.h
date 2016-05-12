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

 private:
  const Hash hash_;
  const std::vector<TreeEntry> entries_;
};
}
}
