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
#include <folly/Range.h>
#include <sys/stat.h>
#include "eden/utils/DirType.h"

namespace facebook {
namespace eden {
namespace fusell {

/**
 * Helper for populating directory listings.
 */
class DirList {
  std::unique_ptr<char[]> buf_;
  char* end_;
  char* cur_;

 public:
  explicit DirList(size_t maxSize);

  DirList(const DirList&) = delete;
  DirList& operator=(const DirList&) = delete;
  DirList(DirList&&) = default;
  DirList& operator=(DirList&&) = default;

  /**
   * Add a new dirent to the list.
   * Returns true on success or false if the list is full.
   */
  bool add(folly::StringPiece name, ino_t inode, dtype_t type, off_t off);

  /**
   * Variant of add() which takes a struct stat.
   * Only st.st_ino and st.st_mode need be filled out.
   */
  bool add(folly::StringPiece name, const struct stat& st, off_t off);

  folly::StringPiece getBuf() const;
};
}
}
}
