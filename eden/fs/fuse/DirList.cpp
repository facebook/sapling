/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/fuse/DirList.h"

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/utils/FsChannelTypes.h"

using folly::StringPiece;

namespace facebook::eden {

FuseDirList::FuseDirList(size_t maxSize)
    : buf_(new char[maxSize]), end_(buf_.get() + maxSize), cur_(buf_.get()) {}

bool FuseDirList::add(StringPiece name, ino_t inode, dtype_t type, off_t off) {
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
  XDCHECK_LE(cur_, end_);
  return true;
}

StringPiece FuseDirList::getBuf() const {
  return StringPiece(buf_.get(), cur_ - buf_.get());
}

std::vector<FuseDirList::ExtractedEntry> FuseDirList::extract() const {
  std::vector<FuseDirList::ExtractedEntry> result;

  char* p = buf_.get();
  while (p != cur_) {
    auto entry = reinterpret_cast<fuse_dirent*>(p);
    result.emplace_back(ExtractedEntry{
        std::string{entry->name, entry->name + entry->namelen},
        entry->ino,
        static_cast<dtype_t>(entry->type),
        static_cast<off_t>(entry->off)});

    p += FUSE_DIRENT_ALIGN(FUSE_NAME_OFFSET + entry->namelen);
  }
  return result;
}

} // namespace facebook::eden

#endif
