/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Try.h>
#include <folly/futures/Future.h>

namespace facebook::eden {

template <typename T>
class ImmediateFuture;

namespace detail {

template <typename T>
struct isImmediateFuture : std::false_type {};

template <typename T>
struct isImmediateFuture<ImmediateFuture<T>> : std::true_type {};

template <typename T, typename enabled = void>
struct continuation_result_impl {
  using type = T;
};

template <typename T>
struct continuation_result_impl<
    T,
    typename std::enable_if_t<folly::isFutureOrSemiFuture<T>::value>> {
  using type = typename T::value_type;
};

template <typename T>
struct continuation_result_impl<
    T,
    typename std::enable_if_t<isImmediateFuture<T>::value>> {
  using type = typename T::value_type;
};

template <typename T>
struct continuation_result_impl<
    T,
    typename std::enable_if_t<folly::isTry<T>::value>> {
  using type = typename T::element_type;
};

template <>
struct continuation_result_impl<void> {
  using type = folly::Unit;
};

template <typename enabled, typename Func, typename... Arg>
struct continuation_result
    : continuation_result_impl<std::invoke_result_t<Func, Arg...>> {};

/**
 * Returns the actual return type of a continuation callback, removing the
 * Future/Try/ImmediateFuture wrapping.
 */
template <typename Func, typename... Arg>
using continuation_result_t = typename continuation_result<
    std::enable_if_t<std::is_invocable_v<Func, Arg...>>,
    Func,
    Arg...>::type;

/**
 * When set, an ImmediateFuture is always holding a SemiFuture.
 *
 * In order to make it easy to reproduce use-after-free bugs in debug/sanitized
 * builds, the ImmediateFuture code can be forced to always hold a SemiFuture,
 * even when immediate values are being passed to it.
 */
constexpr bool kImmediateFutureAlwaysDefer =
    folly::kIsDebug || folly::kIsSanitize;

} // namespace detail
} // namespace facebook::eden
