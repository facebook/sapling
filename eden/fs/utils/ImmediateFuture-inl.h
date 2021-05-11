/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

template <typename T>
template <typename Func>
ImmediateFuture<detail::continuation_result_t<Func, T>>
ImmediateFuture<T>::thenValue(Func&& func) && {
  return std::move(*this).thenTry(
      [func = std::forward<Func>(func)](
          folly::Try<T>&& try_) -> std::invoke_result_t<Func, T> {
        // If try_ doesn't store a value, this will rethrow the exception which
        // will be caught by the thenTry method below.
        return func(std::move(try_).value());
      });
}

template <typename T>
template <typename Func>
ImmediateFuture<detail::continuation_result_t<Func, folly::Try<T>>>
ImmediateFuture<T>::thenTry(Func&& func) && {
  using NewType = detail::continuation_result_t<Func, folly::Try<T>>;
  using RetType = ImmediateFuture<NewType>;

  return std::visit(
      [func = std::forward<Func>(func)](auto&& inner) mutable -> RetType {
        using Type = std::decay_t<decltype(inner)>;
        if constexpr (std::is_same_v<Type, folly::Try<T>>) {
          if (inner.hasValue()) {
            try {
              return func(std::move(inner));
            } catch (std::exception& ex) {
              return folly::Try<NewType>(
                  folly::exception_wrapper(std::current_exception(), ex));
            }
          } else {
            return folly::Try<NewType>(std::move(inner).exception());
          }
        } else {
          return std::move(inner).defer(std::forward<Func>(func));
        }
      },
      std::move(inner_));
}

template <typename T>
T ImmediateFuture<T>::get() && {
  return std::visit(
      [](auto&& inner) -> T {
        using Type = std::decay_t<decltype(inner)>;
        if constexpr (std::is_same_v<Type, folly::Try<T>>) {
          return std::move(inner).value();
        } else {
          return std::move(inner).get();
        }
      },
      std::move(inner_));
}

template <typename T>
folly::Try<T> ImmediateFuture<T>::getTry() && {
  return std::visit(
      [](auto&& inner) -> folly::Try<T> {
        using Type = std::decay_t<decltype(inner)>;
        if constexpr (std::is_same_v<Type, folly::Try<T>>) {
          return std::move(inner);
        } else {
          return std::move(inner).getTry();
        }
      },
      std::move(inner_));
}

template <typename T>
folly::SemiFuture<T> ImmediateFuture<T>::semi() && {
  return std::visit(
      [](auto&& inner) -> folly::SemiFuture<T> { return std::move(inner); },
      std::move(inner_));
}

} // namespace facebook::eden
