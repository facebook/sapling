/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>

namespace facebook {
namespace eden {

bool isValidUtf8(folly::ByteRange str);

/**
 * Returns whether the given string is correctly-encoded UTF-8.
 */
inline bool isValidUtf8(folly::StringPiece str) {
  return isValidUtf8(folly::ByteRange{str});
}

std::string ensureValidUtf8(folly::ByteRange str);

/**
 * Returns a valid UTF-8 encoding of str, with all invalid code points replaced
 * with FFFD, the Unicode replacement character.
 */
inline std::string ensureValidUtf8(folly::StringPiece str) {
  return ensureValidUtf8(folly::ByteRange{str});
}

/**
 * Returns a valid UTF-8 encoding of str, with all invalid code points replaced
 * with FFFD, the Unicode replacement character.
 *
 * This overload avoids a copy in the common case that the given std::string is
 * already valid UTF-8.
 */
inline std::string ensureValidUtf8(std::string&& str) {
  // Avoid a copy in the common case by checking for validity before attempting
  // to re-encode.
  if (isValidUtf8(str)) {
    return std::move(str);
  } else {
    return ensureValidUtf8(str);
  }
}

template <size_t N>
inline std::string ensureValidUtf8(const char (&str)[N]) {
  return ensureValidUtf8(std::string{str, N - 1});
}

} // namespace eden
} // namespace facebook
