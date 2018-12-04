/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "eden/fs/fuse/FileHandleBase.h"

namespace facebook {
namespace eden {

class DirList;

class DirHandle : public FileHandleBase {
 public:
  /**
   * Read directory
   *
   * Send a DirList filled using DirList::add().
   * Send an empty DirList on end of stream.
   */
  virtual folly::Future<DirList> readdir(DirList&& list, off_t off) = 0;
};

} // namespace eden
} // namespace facebook
