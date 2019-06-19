/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
