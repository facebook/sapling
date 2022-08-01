/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Utf8.h"

#include <folly/Unicode.h>

namespace facebook::eden {

std::string ensureValidUtf8(folly::ByteRange str) {
  std::string output;
  output.reserve(str.size());
  const unsigned char* begin = str.begin();
  const unsigned char* const end = str.end();
  while (begin != end) {
    folly::appendCodePointToUtf8(
        folly::utf8ToCodePoint(begin, end, true), output);
  }
  return output;
}

} // namespace facebook::eden
