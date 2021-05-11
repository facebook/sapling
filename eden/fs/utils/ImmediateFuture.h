/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Executor.h>
#include <folly/futures/Future.h>
#include <variant>
#include "eden/fs/utils/ImmediateFuture-pre.h"

namespace facebook::eden {

/**
 * An ImmediateFuture is a small wrapper around either a folly::SemiFuture<T>,
 * or a folly::Try<T>. This allows code to not pay the overhead of
 * folly::SemiFuture for when an immediate value is available. In particular, an
 * ImmediateFuture will not allocate memory in this case.
 */
template <typename T>
class ImmediateFuture {
 public:
  /**
   * The type of the stored value.
   */
  using value_type = T;

  /**
   * Default construct an ImmediateFuture with T's default constructor.
   */
  ImmediateFuture() noexcept(std::is_nothrow_default_constructible_v<T>)
      : inner_{folly::Try<T>{T{}}} {}

  /**
   * Construct an ImmediateFuture with an already constructed value. No
   * folly::SemiFuture will be allocated.
   */
  /* implicit */ ImmediateFuture(folly::Try<T>&& value) noexcept(
      std::is_nothrow_move_constructible_v<folly::Try<T>>)
      : inner_{std::move(value)} {}

  /**
   * Construct an ImmediateFuture with an already constructed value. No
   * folly::SemiFuture will be allocated.
   */
  /* implicit */ ImmediateFuture(T value) noexcept(
      std::is_nothrow_move_constructible_v<folly::Try<T>>)
      : ImmediateFuture{folly::Try<T>{std::move(value)}} {}

  /**
   * Construct an ImmediateFuture with a SemiFuture.
   */
  /* implicit */ ImmediateFuture(folly::SemiFuture<T>&& fut) noexcept(
      std::is_nothrow_move_constructible_v<folly::SemiFuture<T>>)
      : inner_{std::move(fut)} {}

  ~ImmediateFuture() = default;

  ImmediateFuture(const ImmediateFuture<T>&) = delete;
  ImmediateFuture& operator=(const ImmediateFuture<T>&) = delete;

  ImmediateFuture(ImmediateFuture<T>&&) = default;
  ImmediateFuture& operator=(ImmediateFuture<T>&&) = default;

  /**
   * Queue the func continuation once this future is ready.
   *
   * When the ImmediateFuture is an immediate value, the passed in function
   * will be called without waiting. When a SemiFuture value, the function will
   * be called in the same executor as the previous future executor.
   *
   * Func must be a function taking a T as the only argument, its return value
   * must be of a type that an ImmediateFuture can be constructed from
   * (folly::Try, folly::SemiFuture, etc).
   *
   * This method must be called with an rvalue-ref and should no longer be used
   * afterwards:
   *
   *   ImmediateFuture<int> fut{42};
   *   std::move(fut).thenValue([](int value) { return value + 1; });
   */
  template <typename Func>
  ImmediateFuture<detail::continuation_result_t<Func, T>> thenValue(
      Func&& func) &&;

  /**
   * Queue the func continuation once this future is ready.
   *
   * When the ImmediateFuture is an immediate value, the passed in function
   * will be called without waiting. When a SemiFuture value, the function will
   * be called in the same executor as the previous future executor.
   *
   * Func must be a function taking a Try<T> as the only argument, its return
   * value must be of a type that an ImmediateFuture can be constructed from
   * (folly::Try, folly::SemiFuture, etc).
   *
   * This method must be called with an rvalue-ref and should no longer be used
   * afterwards:
   *
   *   ImmediateFuture<int> fut{42};
   *   std::move(fut).thenTry([](Try<int> value) { return *value + 1; });
   */
  template <typename Func>
  ImmediateFuture<detail::continuation_result_t<Func, folly::Try<T>>> thenTry(
      Func&& func) &&;

  /**
   * Build a SemiFuture out of this ImmediateFuture and returns it.
   *
   * The returned semi future can then be executed on an executor with its
   * via() method. When this ImmediateFuture stores an immediate value, this
   * will allocate a new SemiFuture that is ready.
   *
   * Can be used as such:
   *
   *   ImmediateFuture<T> immFut = ...;
   *   folly::Future<T> fut = std::move(fut).semi().via(executor);
   */
  folly::SemiFuture<T> semi() &&;

  /**
   * Wait for the future to complete and return its value or throw its
   * exception.
   *
   * When the future is an immediate value, this returns without waiting.
   */
  T get() &&;

  /**
   * Wait for the future to complete and return the Try value.
   *
   * When the future is an immediate value, this returns without waiting.
   */
  folly::Try<T> getTry() &&;

  bool hasImmediate() const {
    return std::holds_alternative<folly::Try<T>>(inner_);
  }

 private:
  std::variant<folly::Try<T>, folly::SemiFuture<T>> inner_;
};

} // namespace facebook::eden

#include "eden/fs/utils/ImmediateFuture-inl.h"
