/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeNumber.h"

#include <folly/Conv.h>

namespace facebook::eden {

void toAppend(InodeNumber ino, std::string* result) {
  folly::toAppend(ino.getRawValue(), result);
}

void toAppend(InodeNumber ino, folly::fbstring* result) {
  folly::toAppend(ino.getRawValue(), result);
}

} // namespace facebook::eden
