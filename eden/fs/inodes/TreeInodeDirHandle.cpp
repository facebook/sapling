/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInodeDirHandle.h"

#include "Overlay.h"
#include "eden/fs/model/Tree.h"
#include "eden/utils/DirType.h"

namespace facebook {
namespace eden {

TreeInodeDirHandle::TreeInodeDirHandle(std::shared_ptr<TreeInode> inode)
    : inode_(inode) {}

folly::Future<fusell::DirList> TreeInodeDirHandle::readdir(
    fusell::DirList&& list,
    off_t off) {
  // We will be called mulitple times for a given directory read.  The first
  // time through, the off parameter will be 0 to indicate that it is reading
  // from the start.  On subsequent calls it will hold the off value from the
  // last the entry we added to the DirList.  It may actually hold some
  // arbitrary offset if the application is seeking around in the dir stream.
  // Most applications will perform a full scan until we return an empty
  // DirList.
  // We need to return as soon as we have filled the available space in the
  // provided DirList object.

  // The inode of this directory
  auto dir_inode = inode_->getInode();

  // It's pretty complicated to stitch together the directory contents
  // while trying to skip around and respect the offset parameter.
  // Let's go for the simple approach and see if that is good enough.
  //
  // There are three components to the DirList that we need to populate:
  // 1. The "." and ".." entries
  // 2. The overlay entries if we have any
  // 3. The tree entries if we do not have an overlay entry for this dir.
  //
  // We're going to build a vector of this combined information,
  // then we can paginate sanely using the off parameter.

  /** This struct is pretty lightweight and doesn't need to make any
   * heap copies of names as we accumulate the view of the entries */
  struct Entry {
    const char* name;
    dtype_t type;
    /// If 0, look up/assign it based on name
    fuse_ino_t ino;

    Entry(const char* name, dtype_t type, fuse_ino_t ino = 0)
        : name(name), type(type), ino(ino) {}
  };
  folly::fbvector<Entry> entries;

  // Fetch some info now so that we can be more efficient
  // while populating entries.
  auto myname = inode_->getNameMgr()->resolvePathToNode(dir_inode);

  inode_->getContents().withRLock([&](const auto& dir) {
    entries.reserve(2 /* "." and ".." */ + dir.entries.size());

    // Reserved entries for linking to parent and self.
    entries.emplace_back(".", dtype_t::Dir, dir_inode);
    entries.emplace_back("..", dtype_t::Dir, inode_->getParent());

    for (const auto& entry : dir.entries) {
      entries.emplace_back(
          entry.first.value().c_str(), mode_to_dtype(entry.second->mode));
    }
  });

  // And now the easy part: seek to the provided offset and fill up
  // the DirList with the entries that remain.
  auto entry_iter = entries.begin();
  std::advance(entry_iter, off);

  // The stat struct is only used by the fuse machinery to compute the type
  // of the entry so that it can report an appropriate DT_XXX type up to
  // the caller of readdir(), so we zero initialize it and only fill in
  // the type bits of the mode.  The reset are irrelevant and we don't
  // need to waste effort populating them.  We zero out the struct here
  // once and vary just the bits that need to be updated in the loop below.
  // https://www.daemon-systems.org/man/DTTOIF.3.html
  struct stat st;
  memset(&st, 0, sizeof(st));

  while (entry_iter < entries.end()) {
    const auto& entry = *entry_iter;
    st.st_ino = entry.ino;
    st.st_mode = dtype_to_mode(entry.type);

    if (st.st_ino == 0) {
      // We haven't looked up its inode yet, do so now.
      // We defer it from the first pass above in case we have a huge
      // dir and need to paginate through it across several calls into
      // this function.
      auto node = inode_->getNameMgr()->getNodeByName(
          dir_inode, PathComponentPiece(entry.name));
      st.st_ino = node->getNodeId();
    }

    if (!list.add(entry.name, st, ++off)) {
      break;
    }

    ++entry_iter;
  }

  return std::move(list);
}

folly::Future<fusell::Dispatcher::Attr> TreeInodeDirHandle::setattr(
    const struct stat& attr,
    int to_set) {
  folly::throwSystemErrorExplicit(EROFS);
}

folly::Future<folly::Unit> TreeInodeDirHandle::fsyncdir(bool datasync) {
  // We're read-only here, so there is nothing to sync
  return folly::Unit{};
}

folly::Future<fusell::Dispatcher::Attr> TreeInodeDirHandle::getattr() {
  return inode_->getattr();
}
}
}
