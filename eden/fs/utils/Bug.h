/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Conv.h>
#include <folly/lang/ColdClass.h>
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
 *
 * Example uses:
 *
 * Log a message and throw an exception:
 *
 *   EDEN_BUG() << "bad stuff happened";
 *
 * Log a message, but convert the exception to a folly::exception_wrapper()
 * and return it as a folly::Future:
 *
 *   auto bug = EDEN_BUG() << "bad stuff happened";
 *   return folly::makeFuture<InodePtr>(bug.toException());
 *
 * You should only store the return value of EDEN_BUG() in order to call
 * toException() on it.  Storing the return value prevents it from immediately
 * throwing in the EDEN_BUG() statement.
 */
#define EDEN_BUG() ::facebook::eden::EdenBug(__FILE__, __LINE__)

namespace folly {
class exception_wrapper;
}

namespace facebook {
namespace eden {

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
class EdenBug : public folly::ColdClass {
 public:
  EdenBug(const char* file, int lineNumber);
  EdenBug(EdenBug&& other) noexcept;
  EdenBug& operator=(EdenBug&& other) = delete;
  ~EdenBug() noexcept(false);

  /**
   * Append to the bug message.
   */
  template <typename T>
  EdenBug&& operator<<(T&& t) && {
    using folly::toAppend;
    toAppend(std::forward<T>(t), &message_);
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
  bool throwOnDestruction_{true};
  std::string message_;
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
} // namespace eden
} // namespace facebook
