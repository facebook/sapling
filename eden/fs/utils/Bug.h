/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <fmt/core.h>
#include <folly/CppAttributes.h>
#include <folly/futures/Future.h>
#include <atomic>
#include <string>

/**
 * EDEN_BUG() should be used to log logic errors that should not happen unless
 * there is a bug in the code.
 *
 * In debug builds this macro will cause the program to crash.
 * However, in production builds crashing the program is fairly harsh, as this
 * will destroy the client mount points, causing problems for any open programs
 * or shells the user had that were using eden mounts.  Therefore in production
 * builds EDEN_BUG() just logs the error and then throws an exception that can
 * be handled by the calling code.
 *
 * Use XLOG(FATAL) if you want to crash the program even in production builds.
 */
#define EDEN_BUG()                   \
  ::facebook::eden::EdenBugThrow() & \
      ::facebook::eden::EdenBug(__FILE__, __LINE__)

/**
 * EDEN_BUG_FUTURE() is similar to EDEN_BUG() but should be used in context
 * where a Future should be returned.
 *
 * In debug builds this will crash, but in production builds this will return a
 * folly::Future<Type> containing an exception.
 */
#define EDEN_BUG_FUTURE(Type)            \
  ::facebook::eden::EdenBugTry<Type>() & \
      ::facebook::eden::EdenBug(__FILE__, __LINE__)

/**
 * EDEN_BUG_EXCEPTION() is similar to EDEN_BUG() but returns an exception.
 */
#define EDEN_BUG_EXCEPTION()             \
  ::facebook::eden::EdenBugException() & \
      ::facebook::eden::EdenBug(__FILE__, __LINE__)

namespace folly {
class exception_wrapper;
}

namespace facebook::eden {

/**
 * A helper class returned by the EDEN_BUG() macro.
 *
 * toException() can be called to convert it to a folly::exception_wrapper
 * If toException() has not been called, it will throw an exception when it is
 * destroyed.
 *
 * In debug builds EdenBug causes the program to abort rather than throwing or
 * returning an exception.
 */
class EdenBug {
 public:
  FOLLY_COLD EdenBug(const char* file, int lineNumber);
  FOLLY_COLD EdenBug(EdenBug&& other) noexcept;
  EdenBug& operator=(EdenBug&& other) = delete;
  ~EdenBug();

  /**
   * Append to the bug message.
   */
  template <typename T>
  EdenBug&& operator<<(const T& t) && {
    fmt::format_to(std::back_inserter(message_), "{}", t);
    return std::move(*this);
  }

  /**
   * Convert this EdenBug object to a folly::exception_wrapper
   *
   * If toException() is never called on an EdenBug object, it will throw on
   * destruction.
   */
  folly::exception_wrapper toException();

  /**
   * A wrapper for toException().throw_exception(). A typical use of EDEN_BUG()
   * where the bug is captured is actually noreturn, but the compiler can't see
   * that because moved-from EdenBug doesn't throw.
   *
   * To avoid compiler warnings, write:
   *   auto bug = EDEN_BUG() << "...";
   *   bug.throwException();
   */
  [[noreturn]] void throwException();

  /**
   * Prevent EDEN_BUG() from crashing the program, even in debug builds.
   *
   * This is intended to allow unit tests to disable crashing.
   * This generally shouldn't ever be called from normal production code.
   */
  static void acquireDisableCrashLease();
  static void releaseDisableCrashLease();

 private:
  void logError();

  const char* file_;
  int lineNumber_;
  bool processed_{false};
  std::string message_;
};

class EdenBugThrow {
 public:
  // We use operator&() here since it binds with lower precedence than the <<
  // operator used to construct the EdenBug message.
  [[noreturn]] void operator&(EdenBug&& bug) const {
    bug.throwException();
  }
};

template <typename T>
class EdenBugTry {
 public:
  FOLLY_NODISCARD
  folly::Try<T> operator&(EdenBug&& bug) const {
    return folly::Try<T>(bug.toException());
  }
};

class EdenBugException {
 public:
  FOLLY_NODISCARD
  folly::exception_wrapper operator&(EdenBug&& bug) const {
    return bug.toException();
  }
};

/**
 * EdenBugDisabler temporarily disables crashing on EDEN_BUG as long as it
 * exists.
 */
class EdenBugDisabler {
 public:
  EdenBugDisabler();
  ~EdenBugDisabler();

  EdenBugDisabler(const EdenBugDisabler&) = delete;
  EdenBugDisabler operator=(const EdenBugDisabler&) = delete;
};
} // namespace facebook::eden
