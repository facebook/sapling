/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/mercurial/MercurialManifestDirHandle.h"
#include "eden/fuse/Inodes.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

using InodeNameManager = fusell::InodeNameManager;

MercurialManifestDirHandle::MercurialManifestDirHandle(
    fuse_ino_t parent,
    fuse_ino_t ino,
    std::shared_ptr<LocalMercurialRepoAndRev> repo,
    RelativePathPiece path)
    : parent_(parent), ino_(ino), repo_(repo), path_(path) {}

folly::Future<fusell::DirList> MercurialManifestDirHandle::readdir(
    fusell::DirList&& list, off_t off) {
  const auto& listing = repo_->getManifest().getListing(path_.value());
  struct stat st;

  memset(&st, 0, sizeof(st));

  // The first two slots are linkage to parent and self
  constexpr ino_t SELF_ENTRY = 0;
  constexpr ino_t PARENT_ENTRY = 1;
  constexpr ino_t FIRST_FILE_ENTRY = 2;

  if (off == SELF_ENTRY) {
    st.st_ino = ino_;
    if (!list.add(".", st, ++off)) {
      return std::move(list);
    }
  }

  if (off == PARENT_ENTRY) {
    st.st_ino = parent_;
    if (!list.add("..", st, ++off)) {
      return std::move(list);
    }
  }

  // The next range is for the files in this dir
  const auto& files = listing.files;
  // We don't yet know enough to mark this as S_IFLNK or S_IFREG
  st.st_mode = 0;
  while (off - FIRST_FILE_ENTRY < files.size()) {
    const auto& name = files[off - FIRST_FILE_ENTRY];
    auto node =
        InodeNameManager::get()->getNodeByName(ino_, PathComponentPiece(name));
    st.st_ino = node->getNodeId();

    if (!list.add(name.c_str(), st, ++off)) {
      return std::move(list);
    }
  }

  // And the final range is for the dirs
  const auto& dirs = listing.dirs;
  ino_t first_dir_entry = FIRST_FILE_ENTRY + files.size();
  constexpr size_t MAX_PREFETCH = 6;
  size_t n_prefetched = 0;

  // We know enough to flag these as dirs.  Only the type matters, not
  // the permissions because the syscall is only mapping these to
  // DT_XXX (https://www.daemon-systems.org/man/DTTOIF.3.html)
  st.st_mode = S_IFDIR;
  while (off - first_dir_entry < dirs.size()) {
    const auto& name = dirs[off - first_dir_entry];
    PathComponentPiece namepiece(name);
    auto node = InodeNameManager::get()->getNodeByName(ino_, namepiece);
    st.st_ino = node->getNodeId();

    if (n_prefetched++ < MAX_PREFETCH) {
      auto full_name = path_ + namepiece;
      repo_->getManifest().prefetchFileInfoForDir(full_name);
    }
    if (!list.add(name.c_str(), st, ++off)) {
      return std::move(list);
    }
  }
  return std::move(list);
}

folly::Future<fusell::Dispatcher::Attr> MercurialManifestDirHandle::setattr(
    const struct stat&, int) {
  folly::throwSystemErrorExplicit(EROFS,
                                  "cannot set attributes in the manifest");
}

folly::Future<folly::Unit> MercurialManifestDirHandle::fsyncdir(bool) {
  // Nothing to do; this is a read only handle
  return folly::Unit{};
}

folly::Future<fusell::Dispatcher::Attr> MercurialManifestDirHandle::getattr() {
  fusell::Dispatcher::Attr attr;
  attr.st.st_mode = S_IFDIR | 0755;
  attr.st.st_ino = ino_;
  return attr;
}
}
}
