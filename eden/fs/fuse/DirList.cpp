/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/DirList.h"

#include "eden/fs/fuse/FuseTypes.h"

using folly::StringPiece;

namespace facebook {
namespace eden {

DirList::DirList(size_t maxSize)
    : buf_(new char[maxSize]), end_(buf_.get() + maxSize), cur_(buf_.get()) {}

bool DirList::add(StringPiece name, ino_t inode, dtype_t type, off_t off) {
  const size_t avail = end_ - cur_;
  const auto entLength = FUSE_NAME_OFFSET + name.size();
  const auto fullSize = FUSE_DIRENT_ALIGN(entLength);
  if (fullSize > avail) {
    return false;
  }

  fuse_dirent* const dirent = reinterpret_cast<fuse_dirent*>(cur_);
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

} // namespace eden
} // namespace facebook
