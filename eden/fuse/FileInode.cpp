/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Inodes.h"

using namespace folly;

namespace facebook {
namespace eden {
namespace fusell {

FileInode::FileInode(fuse_ino_t ino) : InodeBase(ino) {}

folly::Future<std::string> FileInode::readlink() {
  FUSELL_NOT_IMPL();
}
folly::Future<std::shared_ptr<FileHandle>> FileInode::open(
    const struct fuse_file_info& fi) {
  FUSELL_NOT_IMPL();
}
folly::Future<uint64_t> FileInode::bmap(size_t blocksize, uint64_t idx) {
  FUSELL_NOT_IMPL();
}
}
}
}
