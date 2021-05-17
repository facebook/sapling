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
          folly::Try<T>&& try_) mutable -> std::invoke_result_t<Func, T> {
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
          try {
            return func(std::move(inner));
          } catch (std::exception& ex) {
            return folly::Try<NewType>(
                folly::exception_wrapper(std::current_exception(), ex));
          }
        } else {
          // In the case where Func returns an ImmediateFuture, we need to
          // transform that return value into a SemiFuture so that the return
          // type is a SemiFuture<NewType> and not a
          // SemiFuture<ImmediateFuture<NewType>>.
          using FuncRetType = std::invoke_result_t<Func, folly::Try<T>>;

          auto semiFut = std::move(inner).defer(std::forward<Func>(func));
          if constexpr (detail::isImmediateFuture<FuncRetType>::value) {
            return std::move(semiFut).deferValue(
                [](auto&& immFut) { return std::move(immFut).semi(); });
          } else {
            return semiFut;
          }
        }
      },
      std::move(inner_));
}

template <typename T>
T ImmediateFuture<T>::get(folly::HighResDuration timeout) && {
  return std::visit(
      [timeout](auto&& inner) -> T {
        using Type = std::decay_t<decltype(inner)>;
        if constexpr (std::is_same_v<Type, folly::Try<T>>) {
          return std::move(inner).value();
        } else {
          return std::move(inner).get(timeout);
        }
      },
      std::move(inner_));
}

template <typename T>
folly::Try<T> ImmediateFuture<T>::getTry(folly::HighResDuration timeout) && {
  return std::visit(
      [timeout](auto&& inner) -> folly::Try<T> {
        using Type = std::decay_t<decltype(inner)>;
        if constexpr (std::is_same_v<Type, folly::Try<T>>) {
          return std::move(inner);
        } else {
          return std::move(inner).getTry(timeout);
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
