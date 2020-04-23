/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/Range.h>
#include <sys/stat.h>
#include <memory>
#include "eden/fs/utils/DirType.h"

namespace facebook {
namespace eden {

/**
 * Helper class for populating directory listings.
 */
class DirList {
 public:
  struct ExtractedEntry {
    std::string name;
    uint64_t inode;
    dtype_t type;

    ExtractedEntry(folly::StringPiece name, uint64_t inode, dtype_t type)
        : name{name}, inode{inode}, type{type} {}
  };

  DirList() = default;

  DirList(const DirList&) = delete;
  DirList& operator=(const DirList&) = delete;
  DirList(DirList&&) = default;
  DirList& operator=(DirList&&) = default;

  /**
   * Add a new directory entry to the list.
   */
  void add(folly::StringPiece name, uint64_t inode, dtype_t type);

  const std::vector<ExtractedEntry>& extract() const {
    return list_;
  }

 private:
  std::vector<ExtractedEntry> list_;
};

} // namespace eden
} // namespace facebook
