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

#include <linux/fuse.h>
#include "fuse_headers.h"

using folly::StringPiece;

namespace facebook {
namespace eden {
namespace fusell {

DirList::DirList(size_t maxSize)
    : buf_(new char[maxSize]), end_(buf_.get() + maxSize), cur_(buf_.get()) {}

bool DirList::add(StringPiece name, ino_t inode, dtype_t type, off_t off) {
  // The libfuse APIs unfortunately only accept null terminated strings,
  // so we manually add the fuse_dirent object here rather than using
  // fuse_add_direntry().

  size_t avail = end_ - cur_;
  auto entLength = FUSE_NAME_OFFSET + name.size();
  auto fullSize = FUSE_DIRENT_ALIGN(entLength);
  if (fullSize > avail) {
    return false;
  }

  fuse_dirent* dirent = reinterpret_cast<fuse_dirent*>(cur_);
  dirent->ino = inode;
  dirent->off = off;
  dirent->namelen = name.size();
  dirent->type = static_cast<decltype(dirent->type)>(type);
  memcpy(dirent->name, name.data(), name.size());
  if (fullSize > entLength) {
    // 0 out any padding
    memset(cur_ + entLength, 0, fullSize - entLength);
  }

  cur_ += fullSize;
  DCHECK_LE(cur_, end_);
  return true;
}

bool DirList::add(StringPiece name, const struct stat& st, off_t off) {
  return add(name, st.st_ino, mode_to_dtype(st.st_mode), off);
}

StringPiece DirList::getBuf() const {
  return StringPiece(buf_.get(), cur_ - buf_.get());
}
}
}
}
