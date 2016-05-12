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

#include "eden/fuse/FileHandle.h"

namespace facebook {
namespace eden {
namespace fusell {

class PassThruFileHandle : public FileHandle {
  int fd_;
  fuse_ino_t ino_;

 public:
  explicit PassThruFileHandle(int fd, fuse_ino_t ino);
  folly::Future<folly::Unit> release() override;
  folly::Future<BufVec> read(size_t size, off_t off) override;
  folly::Future<size_t> write(BufVec&& buf, off_t off) override;
  folly::Future<size_t> write(folly::StringPiece data, off_t off) override;
  folly::Future<folly::Unit> flush(uint64_t lock_owner) override;
  folly::Future<folly::Unit> fsync(bool datasync) override;
  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<Dispatcher::Attr> setattr(const struct stat& attr,
                                          int to_set) override;
};
}
}
}
