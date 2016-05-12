/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "DirList.h"
#include "fuse_headers.h"

namespace facebook {
namespace eden {
namespace fusell {

DirList::DirList(size_t maxSize) : buf_(new char[maxSize]) {
  start_ = buf_.get();
  end_ = start_ + maxSize;
  cur_ = start_;
}

bool DirList::add(const char* name, const struct stat& st, off_t off) {
  size_t avail = end_ - cur_;
  size_t needed = fuse_add_direntry(nullptr, cur_, avail, name, &st, off);
  if (needed <= avail) {
    cur_ += needed;
    return true;
  }
  return false;
}

folly::StringPiece DirList::getBuf() const {
  return folly::StringPiece(start_, cur_ - start_);
}
}
}
}
