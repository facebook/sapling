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

InodeBase::~InodeBase() {}

InodeBase::InodeBase(fuse_ino_t ino) : ino_(ino) {
  // Inode numbers generally shouldn't be 0.
  // Older versions of glibc have bugs handling files with an inode number of 0
  DCHECK_NE(ino_, 0);
}

// See Dispatcher::getattr
folly::Future<Dispatcher::Attr> InodeBase::getattr() {
  FUSELL_NOT_IMPL();
}

// See Dispatcher::setattr
folly::Future<Dispatcher::Attr> InodeBase::setattr(const struct stat& attr,
                                                   int to_set) {
  FUSELL_NOT_IMPL();
}

folly::Future<fuse_entry_param> InodeBase::link(
    std::shared_ptr<DirInode>,
    PathComponentPiece) {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> InodeBase::setxattr(folly::StringPiece name,
                                               folly::StringPiece value,
                                               int flags) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::string> InodeBase::getxattr(folly::StringPiece name) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::vector<std::string>> InodeBase::listxattr() {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::removexattr(folly::StringPiece name) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::access(int mask) {
  FUSELL_NOT_IMPL();
}
}
}
}
