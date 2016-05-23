/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MercurialManifestFileInode.h"
#include "MercurialFullManifest.h"
#include "MercurialManifestDirHandle.h"
#include "MercurialManifestFileHandle.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

using namespace folly;

MercurialManifestFileInode::MercurialManifestFileInode(
    std::shared_ptr<LocalMercurialRepoAndRev> repo,
    fuse_ino_t ino,
    fuse_ino_t parent,
    RelativePathPiece path)
    : FileInode(ino), repo_(repo), ino_(ino), parent_(parent), path_(path) {}

Future<fusell::Dispatcher::Attr> MercurialManifestFileInode::getattr() {
  return repo_->getManifest().getFileInfo(path_).then(
      [=](std::shared_ptr<MercurialFullManifest::FileInfo> info) {
        fusell::Dispatcher::Attr attr;

        attr.st.st_mode = info->mode;
        attr.st.st_size = info->size;
        attr.st.st_ino = ino_;

        return attr;
      });
}

folly::Future<std::string> MercurialManifestFileInode::readlink() {
  // Note that we don't need to sanity check the file type here;
  // the kernel will filter out any requests to readlink a plain
  // file, so we can simply focus on delivering the results
  return repo_->getManifest().catFile(path_);
}

folly::Future<std::unique_ptr<fusell::FileHandle>>
MercurialManifestFileInode::open(const struct fuse_file_info&) {
  return repo_->getManifest().catFile(path_).then([=](std::string&& content) {
    return std::make_unique<MercurialManifestFileHandle>(
        shared_from_this(), std::move(content));
  });
}
}
}
