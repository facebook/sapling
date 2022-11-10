/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string_view>

namespace facebook::eden {

constexpr inline bool starts_with(
    std::string_view haystack,
    std::string_view needle) noexcept {
  return needle == haystack.substr(0, needle.size());
}

constexpr inline bool starts_with(
    std::string_view haystack,
    char needle) noexcept {
  const char* data = haystack.data();
  size_t size = haystack.size();
  return size > 0 && *data == needle;
}

constexpr inline bool starts_with(
    std::string_view haystack,
    const char* needle) noexcept {
  return starts_with(haystack, std::string_view{needle});
}

constexpr bool ends_with(
    std::string_view haystack,
    std::string_view needle) noexcept {
  size_t haystack_size = haystack.size();
  size_t needle_size = needle.size();
  return haystack_size >= needle_size &&
      needle == haystack.substr(haystack_size - needle_size);
}

constexpr bool ends_with(std::string_view haystack, char needle) noexcept {
  const char* data = haystack.data();
  size_t size = haystack.size();
  return size > 0 && data[size - 1] == needle;
}

constexpr bool ends_with(
    std::string_view haystack,
    const char* needle) noexcept {
  return ends_with(haystack, std::string_view{needle});
}

struct string_view : std::string_view {
#if __cplusplus <= 202002L
  constexpr bool starts_with(std::string_view sv) const noexcept {
    return eden::starts_with(*this, sv);
  }

  constexpr bool starts_with(char c) const noexcept {
    return eden::starts_with(*this, c);
  }

  constexpr bool starts_with(const char* s) const {
    return eden::starts_with(*this, s);
  }

  constexpr bool ends_with(std::string_view sv) const noexcept {
    return eden::ends_with(*this, sv);
  }

  constexpr bool ends_with(char c) const noexcept {
    return eden::ends_with(*this, c);
  }

  constexpr bool ends_with(const char* s) const {
    return eden::ends_with(*this, s);
  }
#endif
};

} // namespace facebook::eden
