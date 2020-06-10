/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Utf8.h"
#include <folly/Unicode.h>

namespace facebook {
namespace eden {

bool isValidUtf8(folly::ByteRange str) {
  const unsigned char* begin = str.begin();
  const unsigned char* const end = str.end();
  while (begin != end) {
    // TODO: utf8ToCodePoint's signature means we're unable to distinguish
    // between an invalid encoding and an encoding with a replacement character.
    // Fortunately, replacement characters are uncommon.
    if (U'\ufffd' == folly::utf8ToCodePoint(begin, end, true)) {
      return false;
    }
  }
  return true;
}

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

} // namespace eden
} // namespace facebook
