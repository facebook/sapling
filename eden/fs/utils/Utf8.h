/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/Utility.h>

namespace facebook::eden {

namespace detail {
/**
 * Test if the most significant bit is set.
 */
constexpr bool isBitSet(char c, size_t bit) {
  return (folly::to_unsigned(c) & (1u << bit)) == (1u << bit);
}

/**
 * Test if the character is a valid UTF-8 continuation byte.
 *
 * Continuation bytes are of the form 10xxxxxx
 */
constexpr bool isValidContinuation(char c) {
  return isBitSet(c, 7) && !isBitSet(c, 6);
}

constexpr unsigned char clearContinuationBit(char c) {
  return folly::to_unsigned(c) & ~(1u << 7);
}

/**
 * Test if the next num characters are continuation bytes.
 *
 * Set codepoint with the unicode codepoint.
 */
constexpr bool isValidContinuation(
    const char*& begin,
    const char* const end,
    size_t num,
    uint32_t& codepoint) {
  if (begin + num - 1 > end) {
    return false;
  }

  for (size_t i = 0; i < num; i++) {
    auto c = *begin++;
    if (!isValidContinuation(c)) {
      return false;
    }
    codepoint = (codepoint << 6) | clearContinuationBit(c);
  }

  return true;
}
} // namespace detail

/**
 * Returns whether the given string is correctly-encoded UTF-8.
 *
 * This doesn't verify whether the codepoints are actually valid unicode
 * characters.
 */
constexpr bool isValidUtf8(folly::StringPiece str) {
  const char* begin = str.begin();
  const char* const end = str.end();

  while (begin != end) {
    char first = *begin++;
    if (!detail::isBitSet(first, 7)) {
      // ASCII character, nothing to do.
    } else if (!detail::isBitSet(first, 6)) {
      // 10xxxxxx isn't a valid for the first byte.
      return false;
    } else if (!detail::isBitSet(first, 5)) {
      // 110xxxxx: 2 bytes
      uint32_t codepoint = folly::to_unsigned(first) & 0x1F;
      if (!detail::isValidContinuation(begin, end, 1, codepoint)) {
        return false;
      }

      // Is this an overlong encoding?
      if (codepoint < 0x80) {
        return false;
      }
    } else if (!detail::isBitSet(first, 4)) {
      // 1110xxxx: 3 bytes
      uint32_t codepoint = folly::to_unsigned(first) & 0xF;
      if (!detail::isValidContinuation(begin, end, 2, codepoint)) {
        return false;
      }

      // Is this an overlong encoding?
      if (codepoint < 0x800) {
        return false;
      }
    } else if (!detail::isBitSet(first, 3)) {
      // 11110xxx: 4 bytes
      uint32_t codepoint = folly::to_unsigned(first) & 0x7;
      if (!detail::isValidContinuation(begin, end, 3, codepoint)) {
        return false;
      }

      // Is this an overlong encoding?
      if (codepoint < 0x10000) {
        return false;
      }
    } else {
      // 11111xxx isn't ever valid.
      return false;
    }
  }

  return true;
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

} // namespace facebook::eden
