/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include "eden/fs/utils/ImmediateFuture-pre.h"

namespace facebook::eden {

/**
 * An ImmediateFuture is a Future type with similar semantics to folly::Future
 * except that it optimizes the already-fulfilled case by storing a Try<T>
 * inline. This allows code to not pay the allocation and atomic refcounting
 * overhead of folly::SemiFuture when an immediate value is available.
 *
 * Unlike Future and like SemiFuture, ImmediateFuture will never run an attached
 * callback on the thread that fulfills the corresponding Promise.
 *
 * Like Future and unlike SemiFuture, callbacks must handle running immediately.
 * An attached callback may run either immediately or later, when the
 * ImmediateFuture's value is consumed.
 *
 * Like folly::Future and folly::SemiFuture, all methods can throw a
 * folly::FutureInvalid exception if an ImmediateFuture is used after move.
 *
 * When detail::kImmediateFutureAlwaysDefer is set, all ImmediateFuture
 * constructor are pessimized to behave as if constructed from a non-ready
 * SemiFuture.
 */
template <typename T>
class ImmediateFuture {
  static_assert(
      std::is_nothrow_move_constructible_v<T> &&
          std::is_nothrow_move_assignable_v<T>,
      "ImmediateFuture requires T be noexcept-move. "
      "Box with std::unique_ptr if necessary.");

  // Internal implementation requirements:

  // SemiFuture is a pointer-sized, move-only type, and we rely on it
  // being nothrow.
  static_assert(std::is_nothrow_move_constructible_v<folly::SemiFuture<T>>);

  // If T is noexcept-move, Try<T> must also be noexcept-move.
  static_assert(
      std::is_nothrow_move_constructible_v<folly::Try<T>> &&
      std::is_nothrow_move_assignable_v<folly::Try<T>>);

 public:
  /**
   * The type of the stored value.
   */
  using value_type = T;

  /**
   * To match Future and SemiFuture, the default constructor is deleted.
   * In-place construction should use the std::in_place constructor.
   */
  ImmediateFuture() = delete;

  /**
   * Emplace an ImmediateFuture with the constructor arguments.
   */
  template <typename... Args>
  explicit ImmediateFuture(std::in_place_t, Args&&... args) noexcept(
      std::is_nothrow_constructible_v<T, Args&&...>);

  /**
   * Construct an ImmediateFuture with an already constructed value. No
   * folly::SemiFuture will be allocated.
   */
  /* implicit */ ImmediateFuture(folly::Try<T>&& value) noexcept;

  /**
   * Construct an ImmediateFuture with an already constructed value. No
   * folly::SemiFuture will be allocated.
   */
  template <
      typename U = T,
      typename = std::enable_if_t<std::is_constructible_v<folly::Try<T>, U&&>>>
  /* implicit */ ImmediateFuture(U&& value) noexcept(
      std::is_nothrow_constructible_v<folly::Try<T>, U&&>&&
          std::is_nothrow_move_constructible_v<folly::Try<T>>)
      : ImmediateFuture{folly::Try<T>{std::forward<U>(value)}} {}

  /**
   * Construct an ImmediateFuture with a SemiFuture.
   *
   * If the given SemiFuture is ready, the resulting value is moved into and
   * stored inline in this ImmediateFuture.
   *
   * If lazy evaluation of SemiFuture's callbacks is intentional,
   * SemiFutureReadiness::LazySemiFuture can be set to defeat the optimization
   * described above as well as ensuring that ImmediateFuture::isReady always
   * returns false.
   */
  /* implicit */ ImmediateFuture(folly::SemiFuture<T>&& fut) noexcept
      : ImmediateFuture{std::move(fut), SemiFutureReadiness::EagerSemiFuture} {}

  /**
   * Construct an ImmediateFuture with a Future.
   *
   * This constructor has the same semantics as ImmediateFuture{future.semi()}.
   */
  /* implicit */ ImmediateFuture(folly::Future<T>&& fut) noexcept
      : ImmediateFuture{std::move(fut).semi()} {}

  ~ImmediateFuture();

  ImmediateFuture(const ImmediateFuture&) = delete;
  ImmediateFuture& operator=(const ImmediateFuture&) = delete;

  ImmediateFuture(ImmediateFuture&&) noexcept;
  ImmediateFuture& operator=(ImmediateFuture&&) noexcept;

  /**
   * Returns an ImmediateFuture in an empty state. Any attempt to then*() or
   * get*() the returned ImmediateFuture will throw folly::FutureInvalid.
   */
  static ImmediateFuture makeEmpty() noexcept {
    return ImmediateFuture{Empty{}};
  }

  /**
   * Returns whether this future is valid. Returns false if moved-from or if
   * returned by makeEmpty().
   */
  bool valid() const noexcept {
    return kind_ != Kind::Nothing;
  }

  /**
   * Call the func continuation once this future is ready.
   *
   * If this ImmediateFuture already has a value, `func` will be called without
   * waiting. Otherwise, it will be called on the executor on which the end of
   * the Future chain is scheduled with `SemiFuture::via()`.
   *
   * Func must be a function taking a T as the only argument. Its return value
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
   * Call the func continuation once this future is ready.
   *
   * If this ImmediateFuture already has a value, `func` will be called without
   * waiting. Otherwise, it will be called on the executor on which the end of
   * the Future chain is scheduled with `SemiFuture::via()`.
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
   * Call the func continuation once this future is ready.
   *
   * This is a short-hand for:
   *
   *   std::move(fut)
   *     thenTry([](Try<T> value) {
   *       if (value.hasException()) {
   *         return func(value.exception());
   *       }
   *       return value;
   *     });
   */
  template <typename Func>
  ImmediateFuture<T> thenError(Func&& func) &&;

  /**
   * Call func unconditionally once this future is ready and
   * the value/exception is passed through to the resulting Future.
   *
   * Func is like std::function<void()>. If func throws, its exception
   * will be propagated and the original value/exception discarded.
   *
   * This method must be called with an rvalue-ref and should no longer be used
   * afterwards:
   *
   *   ImmediateFuture<int> fut{42};
   *   std::move(fut).ensure([&]() { cleanup(); });
   */
  template <typename Func>
  ImmediateFuture<T> ensure(Func&& func) &&;

  /**
   * Convenience method for ignoring the value and creating an
   * ImmediateFuture<Unit>.
   * Exceptions still propagate.
   */
  ImmediateFuture<folly::Unit> unit() &&;

  /**
   * Returns true if a value is immediately available.
   *
   * That is, if isReady() returns true, calling `thenValue` or `thenTry` is
   * guaranteed to run the callback immediately.
   */
  bool isReady() const;

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
  FOLLY_NODISCARD folly::SemiFuture<T> semi() &&;

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

  /**
   * Wait for the future to complete and return its value or throw its
   * exception.
   *
   * When the future is an immediate value, this returns without waiting.
   *
   * A folly::FutureTimeout will be thrown if the timeout is reached.
   */
  T get(folly::HighResDuration timeout) &&;

  /**
   * Wait for the future to complete and return the Try value.
   *
   * When the future is an immediate value, this returns without waiting.
   *
   * A folly::FutureTimeout will be thrown if the timeout is reached.
   */
  folly::Try<T> getTry(folly::HighResDuration timeout) &&;

  /**
   * Returns true if this ImmediateFuture contains an immediate result.
   *
   * This function is intended for tests -- use isReady() to know whether
   * a value is available now.
   */
  bool debugIsImmediate() const noexcept {
    return kind_ == Kind::Immediate;
  }

 private:
  using Try = folly::Try<T>;
  using SemiFuture = folly::SemiFuture<T>;

  struct Empty {};
  explicit ImmediateFuture(Empty) noexcept;

  /**
   * Define the behavior of the SemiFuture constructor and continuation when
   * dealing with ready SemiFuture.
   */
  enum class SemiFutureReadiness {
    /**
     * At construction time, and at continuation time, the SemiFuture readiness
     * is tested, a ready one will be treated as if the ImmediateFuture was
     * holding an immediate value.
     */
    EagerSemiFuture,
    /**
     * The SemiFuture is never considered ready even if it is. This can be used
     * to force lazyness by ImmediateFuture users. Prefer using
     * makeNotReadyImmediateFuture to obtain a lazy behavior.
     */
    LazySemiFuture,
  };

  ImmediateFuture(SemiFuture fut, SemiFutureReadiness readiness) noexcept;

  friend ImmediateFuture<folly::Unit> makeNotReadyImmediateFuture();

  /**
   * Clear this ImmediateFuture's contents, marking it empty.
   *
   * Any subsequent access to it will throw folly::FutureInvalid.
   */
  void destroy();

  enum class Kind {
    /** Holds an immediate value, immediate_ is valid. */
    Immediate,
    /** Holds a SemiFuture, semi_ is valid. */
    SemiFuture,
    /** Holds a SemiFuture, ImmediateFuture::isReady will always return false,
       semi_ is valid */
    LazySemiFuture,
    /** Doesn't hold anything, neither immediate_ nor semi_ are valid. */
    Nothing,
  };

  // TODO: At the cost of reimplementing parts of Try, we could save a byte or
  // four by merging these tag bits with Try's tag bits, and differentiate
  // between Value, Exception, SemiFuture, and Nothing.
  Kind kind_;

  union {
    Try immediate_;
    SemiFuture semi_;
  };
};

/**
 * Build an ImmediateFuture that is constructed as not ready.
 *
 * Due to not being ready, the returned ImmediateFuture will never execute
 * continuation inline, this can be used to send work to a background thread
 * when desired even if all the data is present in memory and thus the work
 * would otherwise execute inline.
 */
ImmediateFuture<folly::Unit> makeNotReadyImmediateFuture();

/**
 * Build an ImmediateFuture from an error.
 *
 * The ImmediateFuture type must be exclicitely passed in like:
 *
 *   makeImmediateFuture<int>(std::logic_error("Something is wrong!"));
 */
template <typename T, typename E>
typename std::
    enable_if_t<std::is_base_of<std::exception, E>::value, ImmediateFuture<T>>
    makeImmediateFuture(E const& e);

/**
 * Build an ImmediateFuture from an exception wrapper.
 *
 * The ImmediateFuture type must be exclicitely passed in like:
 *
 *   makeImmediateFuture<int>(tryValue.exception());
 */
template <typename T>
ImmediateFuture<T> makeImmediateFuture(folly::exception_wrapper e);

/**
 * Build an ImmediateFuture from func.
 *
 * Exceptions thrown by func will be captured in the returned ImmediateFuture.
 *
 * This is a shorthand for:
 *
 *   ImmediateFuture<folly::Unit>().thenTry([](auto&&) { return func(); });
 *
 * Note that even when kImmediateFutureAlwaysDefer is set, func will be
 * executed eagerly, however, the returned ImmediateFuture will not be ready.
 */
template <typename Func>
auto makeImmediateFutureWith(Func&& func);

/**
 * Run all the passed in ImmediateFuture to completion.
 *
 * The returned ImmediateFuture will complete when all the passed in
 * ImmediateFuture have completed. The returned vector keeps the same ordering
 * as the given vector ImmediateFuture.
 */
template <typename T>
ImmediateFuture<std::vector<folly::Try<T>>> collectAll(
    std::vector<ImmediateFuture<T>> futures);

/**
 * Run all the passed in ImmediateFuture to completion.
 *
 * This behaves similarly to the collectAll from above, but unwraps all the
 * individual folly::Try. In that case, the returned ImmediateFuture will hold
 * the error.
 *
 * Even in the case of errors, the returned ImmediateFuture will only complete
 * when all the passed in ImmediateFuture have completed.
 */
template <typename T>
ImmediateFuture<std::vector<T>> collectAllSafe(
    std::vector<ImmediateFuture<T>> futures);

/**
 * Run all the passed in ImmediateFuture to completion.
 *
 * This has the same behavior as the version taking a vector but returning a
 * tuple of folly::Try.
 */
template <typename... Fs>
ImmediateFuture<
    std::tuple<folly::Try<typename folly::remove_cvref_t<Fs>::value_type>...>>
collectAll(Fs&&... fs);

/**
 * Run all the passed in ImmediateFuture to completion.
 *
 * This behaves similarly to the collectAll from above, but unwraps all the
 * individual folly::Try. In that case, the returned ImmediateFuture will hold
 * the error.
 *
 * Even in the case of errors, the returned ImmediateFuture will only complete
 * when all the passed in ImmediateFuture have completed.
 */
template <typename... Fs>
ImmediateFuture<std::tuple<typename folly::remove_cvref_t<Fs>::value_type...>>
collectAllSafe(Fs&&... fs);

} // namespace facebook::eden

#include "eden/fs/utils/ImmediateFuture-inl.h"
