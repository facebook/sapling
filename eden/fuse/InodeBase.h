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
#include <folly/futures/Future.h>
#include <atomic>
#include "Dispatcher.h"
#include "fuse_headers.h"

namespace facebook {
namespace eden {
namespace fusell {

class InodeBase : public std::enable_shared_from_this<InodeBase> {
  fuse_ino_t ino_;
  // A reference count tracking the outstanding lookups that the kernel
  // has performed on this inode.  This lets us track when we can forget
  // about it.
  std::atomic<uint32_t> nlookup_{1};

 public:
  virtual ~InodeBase();
  explicit InodeBase(fuse_ino_t ino);
  fuse_ino_t getNodeId() const {
    return ino_;
  }

  void incNumLookups(uint32_t count = 1) {
    nlookup_.fetch_add(count, std::memory_order_acq_rel);
  }
  uint32_t decNumLookups(uint32_t count = 1) {
    auto prev = nlookup_.fetch_sub(count, std::memory_order_acq_rel);
    return prev - count;
  }

  // See Dispatcher::getattr
  virtual folly::Future<Dispatcher::Attr> getattr();

  // See Dispatcher::setattr
  virtual folly::Future<Dispatcher::Attr> setattr(const struct stat& attr,
                                                  int to_set);

  virtual folly::Future<folly::Unit> setxattr(folly::StringPiece name,
                                              folly::StringPiece value,
                                              int flags);
  virtual folly::Future<std::string> getxattr(folly::StringPiece name);
  virtual folly::Future<std::vector<std::string>> listxattr();
  virtual folly::Future<folly::Unit> removexattr(folly::StringPiece name);
  virtual folly::Future<folly::Unit> access(int mask);

  /** Return true if Dispatcher should honor a FORGET and free
   * this inode object.  Return false if we should preserve it anyway. */
  virtual bool canForget();
};
}
}
}
