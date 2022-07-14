/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
    Uncopyable() {}
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

TEST(ImmediateFuture, makeImmediateFutureError) {
  auto fut = makeImmediateFuture<int>(std::logic_error("Failure"));
  EXPECT_THROW(std::move(fut).get(), std::logic_error);
}

TEST(ImmediateFuture, collectAllTuple) {
  auto f1 = ImmediateFuture<int>{42};
  auto f2 = ImmediateFuture<float>{42.};

  auto future = collectAll(std::move(f1), std::move(f2));
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).get();
  EXPECT_EQ(std::get<folly::Try<int>>(res).value(), 42);
  EXPECT_EQ(std::get<folly::Try<float>>(res).value(), 42.);
}

TEST(ImmediateFuture, collectAllTupleSemi) {
  auto [promise, semiFut] = folly::makePromiseContract<int>();
  auto f1 = ImmediateFuture<int>{std::move(semiFut)};
  auto f2 = ImmediateFuture<float>{42.};

  auto future = collectAll(std::move(f1), std::move(f2));
  EXPECT_FALSE(future.isReady());

  promise.setValue(42);

  auto res = std::move(future).get();
  EXPECT_EQ(std::get<folly::Try<int>>(res).value(), 42);
  EXPECT_EQ(std::get<folly::Try<float>>(res).value(), 42.);
}

TEST(ImmediateFuture, collectAllTupleSemiReady) {
  auto [promise1, semiFut1] = folly::makePromiseContract<int>();
  auto f1 = ImmediateFuture<int>{std::move(semiFut1)};
  auto [promise2, semiFut2] = folly::makePromiseContract<int>();
  auto f2 = ImmediateFuture<int>{std::move(semiFut2)};

  promise1.setValue(42);
  promise2.setValue(43);

  auto future = collectAll(std::move(f1), std::move(f2));
  EXPECT_FALSE(future.isReady());

  auto res = std::move(future).get(1ms);
  EXPECT_EQ(std::get<0>(res).value(), 42);
  EXPECT_EQ(std::get<1>(res).value(), 43);
}

TEST(ImmediateFuture, collectAllSafeTuple) {
  auto f1 = ImmediateFuture<int>{42};
  auto f2 = ImmediateFuture<float>{
      folly::Try<float>{std::logic_error("Test exception")}};

  auto future = collectAllSafe(std::move(f1), std::move(f2));
  EXPECT_TRUE(future.isReady());

  EXPECT_THROW_RE(std::move(future).get(), std::logic_error, "Test exception");
}

TEST(ImmediateFuture, collectAllSafeTupleError) {
  auto [promise1, semiFut1] = folly::makePromiseContract<int>();
  auto [promise2, semiFut2] = folly::makePromiseContract<int>();

  auto f1 = ImmediateFuture{std::move(semiFut1)};
  auto f2 = ImmediateFuture{std::move(semiFut2)};

  auto future = collectAllSafe(std::move(f1), std::move(f2))
                    .semi()
                    .via(&folly::QueuedImmediateExecutor::instance());
  EXPECT_FALSE(future.isReady());

  promise1.setException(std::logic_error("Test"));
  EXPECT_FALSE(future.isReady());

  promise2.setValue(42);
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).getTry();
  EXPECT_THROW_RE(res.value(), std::logic_error, "Test");
}

TEST(ImmediateFuture, collectAllSafeTupleValid) {
  auto f1 = ImmediateFuture<int>{42};
  auto f2 = ImmediateFuture<float>{42.};

  auto future = collectAllSafe(std::move(f1), std::move(f2));
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).get();
  EXPECT_EQ(std::get<int>(res), 42);
  EXPECT_EQ(std::get<float>(res), 42.);
}

TEST(ImmediateFuture, collectAllSafeVector) {
  std::vector<ImmediateFuture<int>> vec;
  vec.push_back(ImmediateFuture<int>{42});
  vec.push_back(makeImmediateFuture<int>(std::logic_error("Test exception")));

  auto fut = collectAllSafe(std::move(vec));
  EXPECT_TRUE(fut.isReady());

  EXPECT_THROW_RE(std::move(fut).get(), std::logic_error, "Test exception");
}

TEST(ImmediateFuture, collectAllSafeVectorError) {
  auto [promise1, semiFut1] = folly::makePromiseContract<int>();
  auto [promise2, semiFut2] = folly::makePromiseContract<int>();

  std::vector<ImmediateFuture<int>> vec;
  vec.emplace_back(std::move(semiFut1));
  vec.emplace_back(std::move(semiFut2));

  auto future = collectAllSafe(std::move(vec))
                    .semi()
                    .via(&folly::QueuedImmediateExecutor::instance());
  EXPECT_FALSE(future.isReady());

  promise1.setException(std::logic_error("Test"));
  EXPECT_FALSE(future.isReady());

  promise2.setValue(42);
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).getTry();
  EXPECT_THROW_RE(res.value(), std::logic_error, "Test");
}

TEST(ImmediateFuture, collectAllSafeVectorValid) {
  std::vector<ImmediateFuture<int>> vec;
  vec.push_back(ImmediateFuture<int>{42});
  vec.push_back(ImmediateFuture<int>{43});

  auto future = collectAllSafe(std::move(vec));
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).get();
  EXPECT_EQ(res.size(), 2);
  EXPECT_EQ(res[0], 42);
  EXPECT_EQ(res[1], 43);
}

TEST(ImmediateFuture, unit_method) {
  std::vector<ImmediateFuture<int>> vec;
  vec.push_back(ImmediateFuture<int>{42});
  vec.push_back(ImmediateFuture<int>{43});

  auto future = collectAllSafe(std::move(vec)).unit();
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).get();
  EXPECT_EQ(res, folly::unit);
}

TEST(ImmediateFuture, unit_method_error) {
  auto [promise1, semiFut1] = folly::makePromiseContract<int>();
  auto [promise2, semiFut2] = folly::makePromiseContract<int>();

  auto f1 = ImmediateFuture{std::move(semiFut1)};
  auto f2 = ImmediateFuture{std::move(semiFut2)};

  auto future = collectAllSafe(std::move(f1), std::move(f2))
                    .semi()
                    .via(&folly::QueuedImmediateExecutor::instance())
                    .unit();
  EXPECT_FALSE(future.isReady());

  promise1.setException(std::logic_error("Test"));
  EXPECT_FALSE(future.isReady());

  promise2.setValue(42);
  EXPECT_TRUE(future.isReady());

  auto res = std::move(future).getTry();
  EXPECT_THROW_RE(res.value(), std::logic_error, "Test");
}

TEST(ImmediateFuture, thenError) {
  int value = 42;
  ImmediateFuture<int> fortyTwo{value};
  auto exc = std::move(fortyTwo).thenValue(
      [](int) -> int { throw std::logic_error("Test exception"); });
  auto fortyThree = std::move(exc).thenError([](folly::exception_wrapper exc) {
    EXPECT_THROW_RE(exc.throw_exception(), std::logic_error, "Test exception");
    return 43;
  });
  EXPECT_EQ(std::move(fortyThree).get(), 43);
}

TEST(ImmediateFuture, thenErrorVoid) {
  ImmediateFuture<folly::Unit> unitFut{folly::unit};
  auto fut = std::move(unitFut).thenError(
      [](folly::exception_wrapper exc) { exc.throw_exception(); });
  EXPECT_EQ(std::move(fut).get(), folly::unit);
}

TEST(ImmediateFuture, thenErrorSemiValue) {
  auto [promise, semiFut] = folly::makePromiseContract<folly::Unit>();
  ImmediateFuture<folly::Unit> fut{std::move(semiFut)};
  auto thenErrorFut = std::move(fut).thenError(
      [](folly::exception_wrapper exc) { exc.throw_exception(); });
  promise.setValue(folly::unit);
  EXPECT_EQ(std::move(thenErrorFut).get(), folly::unit);
}

TEST(ImmediateFuture, thenErrorSemiError) {
  auto [promise, semiFut] = folly::makePromiseContract<folly::Unit>();
  ImmediateFuture<folly::Unit> fut{std::move(semiFut)};
  auto thenErrorFut =
      std::move(fut).thenError([](folly::exception_wrapper exc) {
        // Re-throw with a different type so we can test that the original
        // exception was caught.
        throw std::runtime_error(folly::exceptionStr(exc).toStdString());
      });
  promise.setException(std::logic_error("Test exception"));
  EXPECT_THROW_RE(
      std::move(thenErrorFut).get(), std::runtime_error, "Test exception");
}
