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

class FileHandle : public FileHandleBase {
 public:
  /**
   * Write data
   *
   * Write should return exactly the number of bytes requested
   * except on error.  An exception to this is when the file has
   * been opened in 'direct_io' mode, in which case the return value
   * of the write system call will reflect the return value of this
   * operation.
   */
  FOLLY_NODISCARD virtual folly::Future<size_t> write(
      BufVec&& buf,
      off_t off) = 0;
  FOLLY_NODISCARD virtual folly::Future<size_t> write(
      folly::StringPiece data,
      off_t off) = 0;
};

} // namespace eden
} // namespace facebook
