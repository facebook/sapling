/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

/*
 * This file contains additional gtest-style check macros to use in unit tests.
 */

#include <folly/Conv.h>
#include <folly/ExceptionString.h>
#include <gtest/gtest.h>
#include <regex>
#include <system_error>

#define TEST_THROW_ERRNO_(statement, errnoValue, fail)         \
  GTEST_AMBIGUOUS_ELSE_BLOCKER_                                \
  if (::facebook::eden::test::CheckResult gtest_result =       \
          ::facebook::eden::test::checkThrowErrno(             \
              [&]() { statement; }, errnoValue, #statement)) { \
  } else                                                       \
  fail(gtest_result.what())

/**
 * Check that a statement throws a std::system_error with the expected errno
 * value.
 */
#define EXPECT_THROW_ERRNO(statement, errnoValue) \
  TEST_THROW_ERRNO_(statement, errnoValue, GTEST_NONFATAL_FAILURE_)
#define ASSERT_THROW_ERRNO(statement, errnoValue) \
  TEST_THROW_ERRNO_(statement, errnoValue, GTEST_FATAL_FAILURE_)

#define TEST_THROW_RE_(statement, exceptionType, pattern, fail)             \
  GTEST_AMBIGUOUS_ELSE_BLOCKER_                                             \
  if (::facebook::eden::test::CheckResult gtest_result =                    \
          ::facebook::eden::test::CheckThrowRegex<exceptionType>::check(    \
              [&]() { statement; }, pattern, #statement, #exceptionType)) { \
  } else                                                                    \
  fail(gtest_result.what())

/**
 * Check that a statement throws the expected exception type, and that the
 * exception message matches the specified regular expression.
 */
#define EXPECT_THROW_RE(statement, exceptionType, pattern) \
  TEST_THROW_RE_(statement, exceptionType, pattern, GTEST_NONFATAL_FAILURE_)
#define ASSERT_THROW_RE(statement, exceptionType, pattern) \
  TEST_THROW_RE_(statement, exceptionType, pattern, GTEST_FATAL_FAILURE_)

namespace facebook {
namespace eden {
namespace test {
/**
 * Helper class for implementing test macros
 */
class CheckResult {
 public:
  explicit CheckResult(bool s) : success_(s) {}

  explicit operator bool() const {
    return success_;
  }
  const char* what() const {
    return message_.c_str();
  }

  template <typename T>
  CheckResult& operator<<(T&& t) {
    folly::toAppend(std::forward<T>(t), &message_);
    return *this;
  }

 private:
  bool success_;
  std::string message_;
};

/**
 * Helper function for implementing EXPECT_THROW
 */
template <typename Fn>
CheckResult checkThrowErrno(Fn&& fn, int errnoValue, const char* statementStr) {
  try {
    fn();
  } catch (const std::system_error& ex) {
    // TODO: POSIX errno values should really use std::generic_category(),
    // but folly throws them with std::system_category() at the moment.
    if (ex.code().category() != std::system_category()) {
      return CheckResult(false)
          << "Expected: " << statementStr << "throws an exception with errno "
          << errnoValue << " (" << std::generic_category().message(errnoValue)
          << ")\nActual: it throws a system_error with category "
          << ex.code().category().name() << ": " << ex.what();
    }
    if (ex.code().value() != errnoValue) {
      return CheckResult(false)
          << "Expected: " << statementStr << "throws an exception with errno "
          << errnoValue << " (" << std::generic_category().message(errnoValue)
          << ")\nActual: it throws errno " << ex.code().value() << ": "
          << ex.what();
    }
    return CheckResult(true);
  } catch (const std::exception& ex) {
    return CheckResult(false)
        << "Expected: " << statementStr << "throws an exception with errno "
        << errnoValue << " (" << std::generic_category().message(errnoValue)
        << ")\nActual: it throws a different exception: "
        << ::folly::exceptionStr(ex);
  } catch (...) {
    return CheckResult(false)
        << "Expected: " << statementStr << "throws an exception with errno "
        << errnoValue << " (" << std::generic_category().message(errnoValue)
        << ")\nActual: it throws a non-exception type";
  }
  return CheckResult(false)
      << "Expected: " << statementStr << "throws an exception with errno "
      << errnoValue << " (" << std::generic_category().message(errnoValue)
      << ")\nActual: it throws nothing";
}

/**
 * Helper function for implementing EXPECT_THROW_RE
 *
 * This has to be implemented as a struct instead of a standalone function so
 * we can specialize it for std::exception below.
 */
template <typename ExType>
struct CheckThrowRegex {
  template <typename Fn>
  static CheckResult check(
      Fn&& fn,
      const char* pattern,
      const char* statementStr,
      const char* excTypeStr) {
    try {
      fn();
    } catch (const ExType& ex) {
      std::regex re(pattern);
      if (!std::regex_search(ex.what(), re)) {
        return CheckResult(false) << "Expected: " << statementStr << "throws a "
                                  << excTypeStr << " with message matching \""
                                  << pattern
                                  << "\"\nActual: message is: " << ex.what();
      }
      return CheckResult(true);
    } catch (const std::exception& ex) {
      return CheckResult(false)
          << "Expected: " << statementStr << "throws a " << excTypeStr
          << ")\nActual: it throws a different exception type: "
          << ::folly::exceptionStr(ex);
    } catch (...) {
      return CheckResult(false) << "Expected: " << statementStr << "throws a "
                                << excTypeStr
                                << ")\nActual: it throws a non-exception type";
    }
    return CheckResult(false) << "Expected: " << statementStr << "throws a "
                              << excTypeStr << ")\nActual: it throws nothing";
  }
};

/**
 * Specialization of CheckThrowRegex() for std::exception
 *
 * This avoids two separate catch blocks both catching std::exception,
 * which gcc complains about with -Werror.
 */
template <>
struct CheckThrowRegex<std::exception> {
  template <typename Fn>
  static CheckResult check(
      Fn&& fn,
      const char* pattern,
      const char* statementStr,
      const char* excTypeStr) {
    try {
      fn();
    } catch (const std::exception& ex) {
      std::regex re(pattern);
      if (!std::regex_search(ex.what(), re)) {
        return CheckResult(false) << "Expected: " << statementStr << "throws a "
                                  << excTypeStr << " with message matching \""
                                  << pattern
                                  << "\"\nActual: message is: " << ex.what();
      }
      return CheckResult(true);
    } catch (...) {
      return CheckResult(false) << "Expected: " << statementStr << "throws a "
                                << excTypeStr
                                << ")\nActual: it throws a non-exception type";
    }
    return CheckResult(false) << "Expected: " << statementStr << "throws a "
                              << excTypeStr << ")\nActual: it throws nothing";
  }
};
}
}
}
