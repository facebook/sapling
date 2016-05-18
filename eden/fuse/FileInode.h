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

#include "InodeBase.h"

namespace facebook {
namespace eden {
namespace fusell {

class FileInode : public InodeBase {
 public:
  explicit FileInode(fuse_ino_t ino);
  // See Dispatcher::readlink
  virtual folly::Future<std::string> readlink();
  virtual folly::Future<std::unique_ptr<FileHandle>> open(
      const struct fuse_file_info& fi);
  virtual folly::Future<uint64_t> bmap(size_t blocksize, uint64_t idx);
};
}
}
}
