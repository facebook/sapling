/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ImmediateFuture.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using namespace std::literals::chrono_literals;

TEST(ImmediateFuture, get) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  EXPECT_EQ(std::move(fortyTwo).get(), value);

  ImmediateFuture<int> fortyTwoFut{folly::makeSemiFuture(value)};
  EXPECT_EQ(std::move(fortyTwo).get(), value);
}

TEST(ImmediateFuture, getTry) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{folly::Try<int>(value)};
  EXPECT_EQ(std::move(fortyTwo).getTry().value(), value);
}

TEST(ImmediateFuture, thenValue) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto fortyThree = std::move(fortyTwo).thenValue([](int v) { return v + 1; });
  auto fortyFour =
      std::move(fortyThree).thenValue([](int v) { return folly::Try{v + 1}; });
  auto fortyFive =
      std::move(fortyFour).thenValue([](const int& v) { return v + 1; });
  auto fortySix = std::move(fortyFive).thenValue([](int&& v) { return v + 1; });
  EXPECT_EQ(std::move(fortySix).get(), 46);
}

TEST(ImmediateFuture, thenValueReturnsImmediateFuture) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto fortyThree = std::move(fortyTwo).thenValue(
      [](int v) -> ImmediateFuture<int> { return v + 1; });
  EXPECT_EQ(std::move(fortyThree).get(), 43);
}

TEST(ImmediateFuture, thenTry) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto fortyThree = std::move(fortyTwo).thenTry([](folly::Try<int> try_) {
    EXPECT_TRUE(try_.hasValue());
    return *try_ + 1;
  });
  auto fortyFour = std::move(fortyThree).thenTry([](folly::Try<int> try_) {
    EXPECT_TRUE(try_.hasValue());
    return folly::Try<int>{*try_ + 1};
  });
  auto fortyFive =
      std::move(fortyFour).thenTry([](const folly::Try<int>& try_) {
        EXPECT_TRUE(try_.hasValue());
        return folly::Try<int>{*try_ + 1};
      });
  auto fortySix = std::move(fortyFive).thenTry([](folly::Try<int>&& try_) {
    EXPECT_TRUE(try_.hasValue());
    return folly::Try<int>{*try_ + 1};
  });
  auto fortySeven = std::move(fortySix).thenTry([](folly::Try<int>&& try_) {
    EXPECT_TRUE(try_.hasValue());
    return folly::makeSemiFuture<int>(*try_ + 1);
  });
  EXPECT_EQ(std::move(fortySeven).get(), 47);
}

TEST(ImmediateFuture, exception) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto fortyThree = std::move(fortyTwo).thenValue(
      [](int) -> int { throw std::logic_error("Test exception"); });
  EXPECT_THROW_RE(
      std::move(fortyThree).get(), std::logic_error, "Test exception");
}

TEST(ImmediateFuture, exceptionContinuation) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto exceptedFut = std::move(fortyTwo)
                         .thenValue([](int) -> int {
                           throw std::logic_error("Test exception");
                         })
                         .thenTry([](folly::Try<int>&& try_) {
                           EXPECT_TRUE(try_.hasException());
                           return std::move(try_);
                         });
  EXPECT_THROW_RE(
      std::move(exceptedFut).get(), std::logic_error, "Test exception");
}

TEST(ImmediateFuture, hasImmediate) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  EXPECT_TRUE(fortyTwo.hasImmediate());
  auto fortyThree = std::move(fortyTwo).thenValue([](int v) { return v + 1; });
  EXPECT_TRUE(fortyThree.hasImmediate());
  auto fortyFour =
      std::move(fortyThree).thenValue([](int v) { return folly::Try{v + 1}; });
  EXPECT_TRUE(fortyFour.hasImmediate());
  auto fortyFive = std::move(fortyFour).thenValue(
      [](int v) { return folly::makeSemiFuture(v + 1); });
  EXPECT_FALSE(fortyFive.hasImmediate());
  EXPECT_EQ(std::move(fortyFive).get(), 45);
}

ImmediateFuture<folly::Unit> unitFunc() {
  return folly::unit;
}

TEST(ImmediateFuture, unit) {
  auto fut = unitFunc();
  EXPECT_TRUE(fut.hasImmediate());

  auto voidFut = std::move(fut).thenValue([](folly::Unit) {});
  EXPECT_TRUE(voidFut.hasImmediate());
}

class Foo {
 public:
  Foo() = delete;
  explicit Foo(int val) : val_(val) {}

  int getVal() const {
    return val_;
  }

  int getNonConstVal() {
    return val_;
  }

  void setVal(int val) {
    val_ = val;
  }

 private:
  int val_;
};

TEST(ImmediateFuture, defaultCtor) {
  ImmediateFuture<Foo> noDefaultCtor{Foo{42}};
  auto fortyThree = std::move(noDefaultCtor).thenValue([](auto&& foo) {
    return foo.getVal() + 1;
  });
  EXPECT_EQ(std::move(fortyThree).get(), 43);

  ImmediateFuture<int> defaultCtor{};
  auto one =
      std::move(defaultCtor).thenValue([](auto&& zero) { return zero + 1; });
  EXPECT_EQ(std::move(one).get(), 1);
}

TEST(ImmediateFuture, semi) {
  ImmediateFuture<int> semi{folly::SemiFuture<int>{42}};
  EXPECT_EQ(std::move(semi).semi().get(), 42);

  ImmediateFuture<int> imm{42};
  EXPECT_EQ(std::move(imm).semi().get(), 42);
}

TEST(ImmediateFuture, mutableLambda) {
  ImmediateFuture<int> fut{42};
  Foo foo{1};
  auto setFooFut = std::move(fut).thenValue(
      [foo](auto&& value) mutable { return value + foo.getNonConstVal(); });
  EXPECT_EQ(std::move(setFooFut).get(), 43);
}

TEST(ImmediateFuture, getTimeout) {
  auto [promise, semiFut] = folly::makePromiseContract<int>();
  ImmediateFuture<int> fut{std::move(semiFut)};
  EXPECT_THROW(std::move(fut).get(0ms), folly::FutureTimeout);
}

TEST(ImmediateFuture, makeImmediateFutureWith) {
  auto fut1 = makeImmediateFutureWith([]() { return 42; });
  EXPECT_TRUE(fut1.hasImmediate());
  EXPECT_EQ(std::move(fut1).get(), 42);

  auto fut2 = makeImmediateFutureWith(
      []() { throw std::logic_error("Test exception"); });
  EXPECT_TRUE(fut2.hasImmediate());
  EXPECT_THROW_RE(std::move(fut2).get(), std::logic_error, "Test exception");

  auto fut3 =
      makeImmediateFutureWith([]() { return folly::makeSemiFuture(42); });
  EXPECT_FALSE(fut3.hasImmediate());
  EXPECT_EQ(std::move(fut3).get(), 42);
}
