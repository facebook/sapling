/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include <folly/ExceptionWrapper.h>
#include <folly/Try.h>

namespace facebook::eden {

namespace detail {

// folly::Try doesn't allow implicit construction from exception_wrapper, so we
// use operator T to construct the return value without explicitly specifying
// its type.
class TryExceptionAdapter {
 public:
  explicit TryExceptionAdapter(folly::exception_wrapper ex)
      : ex_{std::move(ex)} {}

  template <typename T>
  operator folly::Try<T>() && {
    return folly::Try<T>(std::move(ex_));
  }

 private:
  folly::exception_wrapper ex_;
};

template <typename T>
std::optional<TryExceptionAdapter> extractTryValue(folly::Try<T> t, T& out) {
  if (t.hasException()) {
    return TryExceptionAdapter{std::move(t).exception()};
  } else {
    out = std::move(t).value();
    return std::nullopt;
  }
}

} // namespace detail

/**
 * Declare a variable and extract a Try's value into it, or else return with the
 * Try's exception
 *
 * Used in a function that returns a folly::Try, this simplifies monadic
 * composition of Try-returning operations. For example:
 *
 * folly::Try<std::string> getA();
 * folly::Try<int> getB(std::string a);
 *
 * folly::Try<int> foo() {
 *   EDEN_TRY(a, getA());
 *   EDEN_TRY(b, getB(a + "foo"));
 *   return folly::Try<int>{b+1};
 * }
 *
 * The variables a and b are declared in the calling scope based on the Try's
 * value type.  If either getA() or getB() returns an exception, foo()
 * immediately returns a Try containing that exception.
 *
 * Note that the return type of the function in which EDEN_TRY is used doesn't
 * need to match the type of EDEN_TRY's argument, as long as they're both
 * instantiations of folly::Try.
 */
#define EDEN_TRY(out, t)                                         \
  std::remove_reference<decltype(t)>::type::element_type out;    \
  do {                                                           \
    auto ex = ::facebook::eden::detail::extractTryValue(t, out); \
    if (ex.has_value()) {                                        \
      return std::move(ex).value();                              \
    }                                                            \
  } while (false)

} // namespace facebook::eden
