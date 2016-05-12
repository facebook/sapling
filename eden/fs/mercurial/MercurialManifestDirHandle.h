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
#include "LocalMercurialRepoAndRev.h"
#include "eden/fuse/DirHandle.h"

namespace facebook {
namespace eden {

// Represents an opendir()'d handle to a dir in the hg manifest
class MercurialManifestDirHandle : public fusell::DirHandle {
  fuse_ino_t parent_;
  fuse_ino_t ino_;
  std::shared_ptr<LocalMercurialRepoAndRev> repo_;
  RelativePath path_;

 public:
  MercurialManifestDirHandle(
      fuse_ino_t parent,
      fuse_ino_t ino,
      std::shared_ptr<LocalMercurialRepoAndRev> repo,
      RelativePathPiece path);
  folly::Future<fusell::DirList> readdir(fusell::DirList&& list,
                                         off_t off) override;

  folly::Future<fusell::Dispatcher::Attr> setattr(const struct stat& attr,
                                                  int to_set) override;
  folly::Future<folly::Unit> fsyncdir(bool datasync) override;
  folly::Future<fusell::Dispatcher::Attr> getattr() override;
};
}
}
