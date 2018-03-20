/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/FileHandle.h"

using namespace folly;

namespace facebook {
namespace eden {

bool FileHandle::usesDirectIO() const {
  return false;
}
bool FileHandle::preserveCache() const {
  return false;
}
bool FileHandle::isSeekable() const {
  return true;
}

} // namespace eden
} // namespace facebook
