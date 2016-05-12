/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MercurialManifestDirInode.h"
#include "MercurialFullManifest.h"
#include "MercurialManifestDirHandle.h"
#include "MercurialManifestFileInode.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

using namespace folly;
using InodeNameManager = fusell::InodeNameManager;

MercurialManifestDirInode::MercurialManifestDirInode(
    std::shared_ptr<LocalMercurialRepoAndRev> repo,
    fuse_ino_t ino,
    fuse_ino_t parent,
    RelativePathPiece path)
    : DirInode(ino), repo_(repo), ino_(ino), parent_(parent), path_(path) {}

Future<fusell::Dispatcher::Attr> MercurialManifestDirInode::getattr() {
  return repo_->getManifest().getFileInfo(path_).then(
      [=](std::shared_ptr<MercurialFullManifest::FileInfo> info) {
        fusell::Dispatcher::Attr attr;

        attr.st.st_mode = info->mode;
        attr.st.st_size = info->size;
        attr.st.st_ino = ino_;

        return attr;
      });
}

Future<fusell::DirHandle*> MercurialManifestDirInode::opendir(
    const struct fuse_file_info&) {
  repo_->getManifest().prefetchFileInfoForDir(path_);
  return new MercurialManifestDirHandle(parent_, ino_, repo_, path_);
}

Future<std::shared_ptr<fusell::InodeBase>>
MercurialManifestDirInode::getChildByName(PathComponentPiece namepiece) {
  auto full_name = path_ + namepiece;
  return repo_->getManifest()
      .getFileInfo(full_name)
      .then([=](std::shared_ptr<MercurialFullManifest::FileInfo> info) {

        // Note that the type of this is the base; we're going to create
        // a subclass of this type and can't directly return the result
        // of make_shared without the compiler getting angry about the
        // return type of the lambda
        std::shared_ptr<fusell::InodeBase> inode;
        auto node =
            InodeNameManager::get()->getNodeByName(ino_, full_name.basename());

        if (S_ISDIR(info->mode)) {
          inode = std::make_shared<MercurialManifestDirInode>(
              repo_, node->getNodeId(), ino_, full_name);
        } else {
          inode = std::make_shared<MercurialManifestFileInode>(
              repo_, node->getNodeId(), ino_, full_name);
        }
        return inode;
      })
      .onError(
          [=](const std::out_of_range&) -> std::shared_ptr<fusell::InodeBase> {
            // They asked for a file that isn't in the manifest
            throwSystemErrorExplicit(ENOENT);
          });
}
}
}
