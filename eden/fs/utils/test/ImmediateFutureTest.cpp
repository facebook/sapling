/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/ImmediateFuture.h"

#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>

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

TEST(ImmediateFuture, ensureBasic) {
  size_t count = 0;
  auto ensureFn = [&] { count++; };

  ImmediateFuture<int> fortyTwo{42};
  auto fortyThree = std::move(fortyTwo)
                        .thenValue([](int v) { return v + 1; })
                        .ensure(ensureFn);
  auto fortyFour = std::move(fortyThree)
                       .thenValue([](int&& v) { return v + 1; })
                       .ensure(ensureFn);
  EXPECT_EQ(std::move(fortyFour).get(), 44);
  EXPECT_EQ(2, count);
}

TEST(ImmediateFuture, ensureThrowInFuture) {
  size_t count = 0;
  auto ensureFn = [&] { count++; };

  ImmediateFuture<int> fortyTwo{42};
  auto fortyThree = std::move(fortyTwo)
                        .thenValue([](int v) { return v + 1; })
                        .ensure(ensureFn);
  auto bad = std::move(fortyThree)
                 .thenValue([](int) { throw std::runtime_error("ensure"); })
                 .ensure(ensureFn);
  EXPECT_THROW(std::move(bad).get(), std::runtime_error);
  EXPECT_EQ(2, count);
}

TEST(ImmediateFuture, ensureThrowInFunc) {
  size_t count = 0;
  auto ensureFn = [&] { count++; };
  auto badEnsureFn = [] { throw std::runtime_error("ensure"); };

  ImmediateFuture<int> fortyTwo{42};
  auto bad = std::move(fortyTwo)
                 .thenValue([](int v) { return v + 1; })
                 .ensure(badEnsureFn)
                 .ensure(ensureFn);
  EXPECT_THROW(std::move(bad).get(), std::runtime_error);
  EXPECT_EQ(1, count);
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

ImmediateFuture<folly::Unit> unitFunc() {
  return folly::unit;
}

TEST(ImmediateFuture, unit) {
  auto fut = unitFunc();
  EXPECT_TRUE(fut.isReady());

  auto voidFut = std::move(fut).thenValue([](folly::Unit) {});
  EXPECT_TRUE(voidFut.isReady());
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
  EXPECT_TRUE(fut1.isReady());
  EXPECT_EQ(std::move(fut1).get(), 42);

  auto fut2 = makeImmediateFutureWith(
      []() { throw std::logic_error("Test exception"); });
  EXPECT_TRUE(fut2.isReady());
  EXPECT_THROW_RE(std::move(fut2).get(), std::logic_error, "Test exception");

  auto fut3 =
      makeImmediateFutureWith([]() { return folly::makeSemiFuture(42); });
  EXPECT_TRUE(fut3.isReady());
  EXPECT_EQ(std::move(fut3).get(), 42);

  auto [p, sf] = folly::makePromiseContract<int>();
  auto fut4 = makeImmediateFutureWith(
      [sf = std::move(sf)]() mutable { return std::move(sf); });
  EXPECT_FALSE(fut4.isReady());
  p.setValue(42);
  EXPECT_FALSE(fut4.isReady());
  EXPECT_EQ(std::move(fut4).get(), 42);
}

TEST(ImmediateFuture, isReady_from_value) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  EXPECT_TRUE(fortyTwo.isReady());
}

TEST(ImmediateFuture, isReady_from_completed_SemiFuture) {
  auto semi = folly::makeSemiFuture<int>(10);
  auto imm = ImmediateFuture<int>{std::move(semi)};
  EXPECT_TRUE(imm.isReady());
  EXPECT_EQ(10, std::move(imm).get());
}

TEST(ImmediateFuture, ready_ImmediateFuture_thenValue_is_also_ready) {
  auto semi = folly::makeSemiFuture<int>(10);
  EXPECT_TRUE(semi.isReady());
  auto imm = ImmediateFuture<int>{std::move(semi)};
  EXPECT_TRUE(imm.isReady());
  auto then =
      std::move(imm).thenValue([](int i) -> ImmediateFuture<int> { return i; });
  EXPECT_TRUE(then.isReady());
}

TEST(
    ImmediateFuture,
    ImmediateFuture_does_not_run_SemiFuture_callbacks_until_scheduled_on_executor) {
  bool run = false;
  auto semi = folly::makeSemiFuture<int>(10).deferValue([&](int x) {
    run = true;
    return x + 10;
  });
  EXPECT_FALSE(semi.isReady());
  auto imm = ImmediateFuture<int>{std::move(semi)};
  EXPECT_FALSE(imm.isReady());
  EXPECT_FALSE(run);
  EXPECT_EQ(20, std::move(imm).get());
  EXPECT_TRUE(run);
}

TEST(ImmediateFuture, collectAllImmediate) {
  std::vector<ImmediateFuture<int>> vec;
  vec.push_back(ImmediateFuture<int>{42});
  vec.push_back(ImmediateFuture<int>{43});

  auto fut = collectAll(std::move(vec));
  EXPECT_TRUE(fut.isReady());
  auto res = std::move(fut).get();
  EXPECT_EQ(*res[0], 42);
  EXPECT_EQ(*res[1], 43);
}

TEST(ImmediateFuture, collectAllSemi) {
  std::vector<ImmediateFuture<int>> vec;

  auto [promise1, semiFut1] = folly::makePromiseContract<int>();
  vec.push_back(ImmediateFuture<int>{std::move(semiFut1)});

  auto [promise2, semiFut2] = folly::makePromiseContract<int>();
  vec.push_back(ImmediateFuture<int>{std::move(semiFut2)});

  auto fut = collectAll(std::move(vec));
  EXPECT_FALSE(fut.isReady());

  promise1.setValue(42);
  promise2.setValue(43);

  auto res = std::move(fut).get();
  EXPECT_EQ(*res[0], 42);
  EXPECT_EQ(*res[1], 43);
}

TEST(ImmediateFuture, collectAllMixed) {
  std::vector<ImmediateFuture<int>> vec;

  auto [promise, semiFut] = folly::makePromiseContract<int>();
  vec.push_back(ImmediateFuture<int>{std::move(semiFut)});
  vec.push_back(ImmediateFuture<int>{43});

  auto fut = collectAll(std::move(vec));
  EXPECT_FALSE(fut.isReady());

  promise.setValue(42);

  auto res = std::move(fut).get();
  EXPECT_EQ(*res[0], 42);
  EXPECT_EQ(*res[1], 43);
}

TEST(ImmediateFuture, collectUncopyable) {
  struct Uncopyable {
    Uncopyable(Uncopyable&&) = default;
    Uncopyable(const Uncopyable&) = delete;

    Uncopyable& operator=(Uncopyable&&) = default;
    Uncopyable& operator=(const Uncopyable&) = delete;
  };
  std::vector<ImmediateFuture<Uncopyable>> vec;
  vec.push_back(Uncopyable{});

  auto fut = collectAll(std::move(vec));
  EXPECT_TRUE(fut.isReady());
}

TEST(ImmediateFuture, collectAllOrdering) {
  std::vector<ImmediateFuture<int>> vec;

  auto [promise, semiFut] = folly::makePromiseContract<int>();
  vec.push_back(ImmediateFuture<int>{std::move(semiFut)});
  vec.push_back(ImmediateFuture<int>{43});

  auto fut = collectAll(std::move(vec));
  EXPECT_FALSE(fut.isReady());

  promise.setValue(42);

  // Despite semiFut having completed after the second ImmediateFuture, it
  // should still be first in the returned vector.
  auto res = std::move(fut).get();
  EXPECT_EQ(*res[0], 42);
  EXPECT_EQ(*res[1], 43);
}
