/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Try.h"

#include <functional>

#include <folly/Try.h>
#include <folly/portability/GTest.h>

#include "eden/common/utils/Match.h"

using namespace facebook::eden;

namespace {

using TryProvider = std::function<folly::Try<int>()>;

TryProvider getTryProvider(folly::Try<int> t, std::size_t& invocationCount) {
  return [t, &invocationCount]() {
    ++invocationCount;
    return t;
  };
}

// Counts how many times it was copied.
class CopyCounter {
 public:
  CopyCounter() : numCopies{0} {}
  CopyCounter(const CopyCounter& other) noexcept
      : numCopies{other.numCopies + 1} {}
  CopyCounter(CopyCounter&& other) noexcept : numCopies{other.numCopies} {}

  CopyCounter& operator=(const CopyCounter& other) noexcept {
    numCopies = other.numCopies + 1;
    return *this;
  }

  CopyCounter& operator=(CopyCounter&& other) noexcept {
    numCopies = other.numCopies;
    return *this;
  }

  std::size_t numCopies;
};

TEST(TryTest, returns_value) {
  std::size_t invocationCount = 0;
  auto fn = getTryProvider(folly::Try<int>{42}, invocationCount);

  auto result = [&]() -> folly::Try<int> {
    EDEN_TRY(value, fn());
    return folly::Try<int>(value);
  }();

  EXPECT_FALSE(result.hasException());
  EXPECT_EQ(42, result.value());

  // Ensure we don't evaluate the macro's argument multiple times, in case it's
  // a function which may have side-effects.
  EXPECT_EQ(1, invocationCount);
}

TEST(TryTest, returns_exception) {
  std::size_t invocationCount = 0;
  auto fn = getTryProvider(
      folly::Try<int>{
          folly::exception_wrapper{std::runtime_error{"can't do the thing"}}},
      invocationCount);

  auto result = [&]() -> folly::Try<int> {
    EDEN_TRY(value, fn());
    return folly::Try<int>(value);
  }();

  EXPECT_TRUE(result.hasException());
  EXPECT_NE(
      std::string::npos, result.exception().what().find("can't do the thing"));
  EXPECT_EQ(1, invocationCount);
}

TEST(TryTest, can_move_try) {
  auto tryCopyCounter = folly::Try<CopyCounter>(CopyCounter());
  EXPECT_EQ(0, tryCopyCounter.value().numCopies);

  auto tryNumCopies =
      [tryCopyCounter =
           std::move(tryCopyCounter)]() mutable -> folly::Try<std::size_t> {
    EDEN_TRY(copyCounter, std::move(tryCopyCounter));
    return folly::Try<std::size_t>{copyCounter.numCopies};
  }();

  // We moved tryCopyCounter into TRY, so the macro shouldn't result in a copy
  // of the value within the folly::Try.
  EXPECT_EQ(0, tryNumCopies.value());
}

} // namespace
