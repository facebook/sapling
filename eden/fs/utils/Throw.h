/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/ranges.h>

namespace facebook::eden {

namespace detail {

/**
 * Deduplicate the codegen for exception allocation and __cxa_throw /
 * _CxxThrowException.
 */
template <typename E>
[[noreturn]] void throw_dedup(const char* what) {
  throw E{what};
}

/**
 * Deduplicate the codegen for exception allocation and __cxa_throw /
 * _CxxThrowException.
 */
template <typename E>
[[noreturn]] void throw_dedup(const std::string& what) {
  throw E{what};
}

} // namespace detail

/**
 * `throw_<runtime_error>(a, b, c)` is equivalent to `throw
 * std::runtime_error(concat(a, b, c))` where `concat` concatenates
 * fmt::to_string applied to every argument.
 *
 * May be very slightly more efficient than `throwf` because it only supports
 * concatenation and does not parse a format string.
 */
template <typename E, typename... T>
[[noreturn]] void throw_(T&&... args) {
  detail::throw_dedup<E>(fmt::to_string(
      fmt::join(std::make_tuple<T&&...>(std::forward<T>(args)...), "")));
}

/**
 * `throwf<runtime_error>("error: {}", x)` is equivalent to
 * `throw std::runtime_error(fmt::format("error: {}", x))` but shorter and
 * generates less code.
 */
template <typename E, typename... T>
[[noreturn]] void throwf(fmt::format_string<T...> fmt, T&&... args) {
  detail::throw_dedup<E>(fmt::format(std::move(fmt), std::forward<T>(args)...));
}

} // namespace facebook::eden
