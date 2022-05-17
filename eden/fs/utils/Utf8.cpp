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
    // codePointToUtf8 returns a std::string which is inefficient for something
    // that always fits in 32 bits, but with SSO it probably never allocates at
    // least.
    output += folly::codePointToUtf8(folly::utf8ToCodePoint(begin, end, true));
  }
  return output;
}

} // namespace facebook::eden
