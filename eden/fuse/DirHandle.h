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
#include "DirList.h"
#include "FileHandleBase.h"

namespace facebook {
namespace eden {
namespace fusell {

class DirHandle : public FileHandleBase {
 public:
  /**
   * Read directory
   *
   * Send a DirList filled using DirList::add().
   * Send an empty DirList on end of stream.
   */
  virtual folly::Future<DirList> readdir(DirList&& list, off_t off) = 0;

  /**
   * Release an open directory
   *
   * For every opendir call there will be exactly one releasedir
   * call.
   */
  virtual folly::Future<folly::Unit> releasedir();

  /**
   * Synchronize directory contents
   *
   * If the datasync parameter is non-zero, then only the directory
   * contents should be flushed, not the meta data.
   *
   * @param datasync flag indicating if only data should be flushed
   */
  virtual folly::Future<folly::Unit> fsyncdir(bool datasync) = 0;
};
}
}
}
