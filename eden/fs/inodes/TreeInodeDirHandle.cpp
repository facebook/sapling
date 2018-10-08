/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInodeDirHandle.h"

#include "Overlay.h"
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/DirType.h"

namespace facebook {
namespace eden {

TreeInodeDirHandle::TreeInodeDirHandle(TreeInodePtr inode) : inode_(inode) {}

folly::Future<DirList> TreeInodeDirHandle::readdir(DirList&& list, off_t off) {
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
  auto dir_inode = inode_->getNodeId();

  // It's pretty complicated to stitch together the directory contents
  // while trying to skip around and respect the offset parameter.
  // Let's go for the simple approach and see if that is good enough.
  //
  // `off` is the index into a synthesized list of entries:
  // 1. The "." and ".." entries
  // 3. The TreeInode's entries in order.
  //
  // We're going to build a vector of this combined information,
  // then we can paginate sanely using the off parameter.

  /** This struct is pretty lightweight and doesn't need to make any
   * heap copies of names as we accumulate the view of the entries */
  struct Entry {
    // This must not contain any embedded nuls.
    folly::StringPiece name;
    dtype_t type;
    /// If 0, look up/assign it based on name
    InodeNumber ino;

    Entry(
        folly::StringPiece name,
        dtype_t type,
        InodeNumber ino = InodeNumber{})
        : name(name), type(type), ino(ino) {}
  };
  folly::fbvector<Entry> entries;

  {
    auto dir = inode_->getContents().rlock();
    entries.reserve(2 /* "." and ".." */ + dir->entries.size());

    // Reserved entries for linking to parent and self.
    entries.emplace_back(".", dtype_t::Dir, dir_inode);
    // It's okay to query the parent without the rename lock held because, if
    // readdir is racing with rename, the results are unspecified anyway.
    // http://pubs.opengroup.org/onlinepubs/007908799/xsh/readdir.html
    auto parent = inode_->getParentRacy();
    // For the root of the mount point, just add its own inode ID as its parent.
    // FUSE seems to overwrite the parent inode number on the root dir anyway.
    auto parent_inode = parent ? parent->getNodeId() : dir_inode;
    entries.emplace_back("..", dtype_t::Dir, parent_inode);

    for (const auto& entry : dir->entries) {
      entries.emplace_back(entry.first.value(), entry.second.getDtype());
    }
  }

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
  struct stat st = {};

  while (entry_iter < entries.end()) {
    const auto& entry = *entry_iter;

    if (entry.ino.hasValue()) {
      st.st_ino = entry.ino.get();
    } else {
      // We haven't looked up its inode yet, do so now.
      // We defer it from the first pass above in case we have a huge
      // dir and need to paginate through it across several calls into
      // this function.
      st.st_ino =
          inode_->getChildInodeNumber(PathComponentPiece{entry.name}).get();
    }
    st.st_mode = dtype_to_mode(entry.type);

    if (!list.add(entry.name, st, ++off)) {
      break;
    }

    ++entry_iter;
  }
  inode_->updateAtime();

  return std::move(list);
}

folly::Future<folly::Unit> TreeInodeDirHandle::fsyncdir(bool /*datasync*/) {
  // We're read-only here, so there is nothing to sync
  return folly::unit;
}
} // namespace eden
} // namespace facebook
