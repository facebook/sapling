/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/InodeNumber.h"

#include <folly/Conv.h>
#include <ostream>

namespace facebook {
namespace eden {

std::ostream& operator<<(std::ostream& os, InodeNumber ino) {
  return os << ino.getRawValue();
}

void toAppend(InodeNumber ino, std::string* result) {
  folly::toAppend(ino.getRawValue(), result);
}

void toAppend(InodeNumber ino, folly::fbstring* result) {
  folly::toAppend(ino.getRawValue(), result);
}

} // namespace eden
} // namespace facebook
