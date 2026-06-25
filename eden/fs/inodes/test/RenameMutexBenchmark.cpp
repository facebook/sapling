/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * RenameMutexBenchmark — microbenchmarks comparing the candidate
 * primitives for `EdenMount::renameMutex_`:
 *
 *   1. `folly::SharedMutex`              — the original primitive used
 *                                          before the coroutine
 *                                          migration. Sync only.
 *   2. `folly::coro::SharedMutexFair`    — the all-coroutine primitive
 *                                          considered during the
 *                                          migration. Coroutine acquire
 *                                          is native; sync acquire bridges
 *                                          via `folly::coro::blockingWait`.
 *   3. `HybridRenameMutex` (this file)   — the design we landed: a
 *                                          `folly::SharedMutex` underneath
 *                                          (sync acquire = direct call,
 *                                          zero overhead vs raw
 *                                          SharedMutex) plus a
 *                                          coroutine-aware wrapper with a
 *                                          `folly::coro::Baton` wait
 *                                          queue and an atomic counter
 *                                          short-circuit so sync
 *                                          unlockers skip the
 *                                          waiter-list lock when no
 *                                          coroutines are waiting.
 *
 * The HybridRenameMutex implementation in `EdenMount.{h,cpp}` is the
 * production version. We reproduce a minimal inline version here so the
 * benchmark stays self-contained (the BUCK rule only depends on
 * `folly`, not on the EdenFS inodes target).
 *
 * See the diff Test Plan for measurements.
 */

#include <folly/SharedMutex.h>
#include <folly/Synchronized.h>
#include <folly/coro/Baton.h>
#include <folly/coro/BlockingWait.h>
#include <folly/coro/Invoke.h>
#include <folly/coro/Mutex.h>
#include <folly/coro/SharedMutex.h>
#include <folly/coro/Task.h>
#include <folly/executors/CPUThreadPoolExecutor.h>

#include <benchmark/benchmark.h>

#include <atomic>
#include <deque>
#include <memory>

namespace {

/**
 * Inline copy of `EdenMount::HybridRenameMutex` so the benchmark can run
 * without depending on the EdenFS inodes target. The behavior is
 * intentionally identical to the production class (see
 * `fbcode/eden/fs/inodes/EdenMount.h` for the canonical version).
 */
class HybridRenameMutex {
 public:
  using Token = folly::SharedMutex::Token;

  HybridRenameMutex() = default;
  HybridRenameMutex(const HybridRenameMutex&) = delete;
  HybridRenameMutex& operator=(const HybridRenameMutex&) = delete;

  // ---- Sync exclusive ----
  void lock() {
    inner_.lock();
  }
  void unlock() {
    inner_.unlock();
    // See EdenMount.cpp::HybridRenameMutex::unlock for the formal
    // memory-model rationale for the fence.
    std::atomic_thread_fence(std::memory_order_seq_cst);
    if (numCoroWaiters_.load(std::memory_order_seq_cst) > 0) {
      wakeAllSharedAndOneExclusive();
    }
  }
  bool try_lock() {
    return inner_.try_lock();
  }

  // ---- Sync shared (Token) ----
  void lock_shared(Token& token) {
    inner_.lock_shared(token);
  }
  void unlock_shared(Token& token) {
    inner_.unlock_shared(token);
    std::atomic_thread_fence(std::memory_order_seq_cst);
    if (numCoroWaiters_.load(std::memory_order_seq_cst) > 0) {
      wakeOneWaiter();
    }
  }
  bool try_lock_shared(Token& token) {
    return inner_.try_lock_shared(token);
  }

  // ---- Coroutine exclusive ----
  folly::coro::Task<void> co_lock() {
    while (true) {
      if (try_lock()) {
        co_return;
      }
      auto baton = std::make_unique<folly::coro::Baton>();
      auto* batonPtr = baton.get();
      {
        auto waiters = exclusiveWaiters_.wlock();
        if (try_lock()) {
          co_return;
        }
        waiters->push_back(std::move(baton));
        numCoroWaiters_.fetch_add(1, std::memory_order_seq_cst);
        std::atomic_thread_fence(std::memory_order_seq_cst);
        // Second re-check after enrollment — closes the lost-wakeup
        // window if unlock happened between our first try_lock and
        // our enrollment.
        if (try_lock()) {
          waiters->pop_back();
          numCoroWaiters_.fetch_sub(1, std::memory_order_seq_cst);
          co_return;
        }
      }
      co_await *batonPtr;
      numCoroWaiters_.fetch_sub(1, std::memory_order_seq_cst);
    }
  }

  // ---- Coroutine shared ----
  folly::coro::Task<void> co_lock_shared(Token& token) {
    while (true) {
      if (try_lock_shared(token)) {
        co_return;
      }
      auto baton = std::make_unique<folly::coro::Baton>();
      auto* batonPtr = baton.get();
      {
        auto waiters = sharedWaiters_.wlock();
        if (try_lock_shared(token)) {
          co_return;
        }
        waiters->push_back(std::move(baton));
        numCoroWaiters_.fetch_add(1, std::memory_order_seq_cst);
        std::atomic_thread_fence(std::memory_order_seq_cst);
        if (try_lock_shared(token)) {
          waiters->pop_back();
          numCoroWaiters_.fetch_sub(1, std::memory_order_seq_cst);
          co_return;
        }
      }
      co_await *batonPtr;
      numCoroWaiters_.fetch_sub(1, std::memory_order_seq_cst);
    }
  }

 private:
  void wakeOneWaiter() {
    {
      auto waiters = exclusiveWaiters_.wlock();
      if (!waiters->empty()) {
        auto baton = std::move(waiters->front());
        waiters->pop_front();
        baton->post();
        return;
      }
    }
    auto waiters = sharedWaiters_.wlock();
    if (!waiters->empty()) {
      auto baton = std::move(waiters->front());
      waiters->pop_front();
      baton->post();
    }
  }

  void wakeAllSharedAndOneExclusive() {
    {
      auto waiters = sharedWaiters_.wlock();
      while (!waiters->empty()) {
        auto baton = std::move(waiters->front());
        waiters->pop_front();
        baton->post();
      }
    }
    auto waiters = exclusiveWaiters_.wlock();
    if (!waiters->empty()) {
      auto baton = std::move(waiters->front());
      waiters->pop_front();
      baton->post();
    }
  }

  folly::SharedMutex inner_;
  std::atomic<size_t> numCoroWaiters_{0};
  folly::Synchronized<std::deque<std::unique_ptr<folly::coro::Baton>>>
      sharedWaiters_;
  folly::Synchronized<std::deque<std::unique_ptr<folly::coro::Baton>>>
      exclusiveWaiters_;
};

// =====================================================================
// Uncontended exclusive lock/unlock — one thread
// =====================================================================

static void BM_SharedMutex_LockUnlock(benchmark::State& state) {
  folly::SharedMutex m;
  for (auto _ : state) {
    m.lock();
    m.unlock();
  }
}
BENCHMARK(BM_SharedMutex_LockUnlock);

static void BM_SharedMutexFair_SyncLockUnlock(benchmark::State& state) {
  folly::coro::SharedMutexFair m;
  for (auto _ : state) {
    folly::coro::blockingWait(m.co_scoped_lock());
  }
}
BENCHMARK(BM_SharedMutexFair_SyncLockUnlock);

static void BM_HybridRenameMutex_SyncLockUnlock(benchmark::State& state) {
  HybridRenameMutex m;
  for (auto _ : state) {
    m.lock();
    m.unlock();
  }
}
BENCHMARK(BM_HybridRenameMutex_SyncLockUnlock);

// =====================================================================
// Uncontended shared lock_shared/unlock_shared — one thread
// =====================================================================

static void BM_SharedMutex_LockSharedUnlockShared_NoToken(
    benchmark::State& state) {
  folly::SharedMutex m;
  for (auto _ : state) {
    m.lock_shared();
    m.unlock_shared();
  }
}
BENCHMARK(BM_SharedMutex_LockSharedUnlockShared_NoToken);

static void BM_SharedMutex_LockSharedUnlockShared_Token(
    benchmark::State& state) {
  folly::SharedMutex m;
  folly::SharedMutex::Token token;
  for (auto _ : state) {
    m.lock_shared(token);
    m.unlock_shared(token);
  }
}
BENCHMARK(BM_SharedMutex_LockSharedUnlockShared_Token);

static void BM_HybridRenameMutex_LockSharedUnlockShared(
    benchmark::State& state) {
  HybridRenameMutex m;
  HybridRenameMutex::Token token;
  for (auto _ : state) {
    m.lock_shared(token);
    m.unlock_shared(token);
  }
}
BENCHMARK(BM_HybridRenameMutex_LockSharedUnlockShared);

// =====================================================================
// Coroutine acquire-release round-trip — single thread, executor
// scheduled. One executor amortized across all iterations.
// =====================================================================

static void BM_SharedMutexFair_CoroLockUnlock(benchmark::State& state) {
  folly::CPUThreadPoolExecutor exec(1);
  folly::coro::SharedMutexFair m;
  // Amortized: drive a single Task that loops over iterations, instead
  // of scheduling a fresh Task per iteration (which would dominate
  // measurements with executor scheduling cost).
  folly::coro::blockingWait(
      folly::coro::co_invoke([&]() -> folly::coro::Task<void> {
        for (auto _ : state) {
          co_await m.co_scoped_lock();
        }
        co_return;
      }).scheduleOn(&exec));
}
BENCHMARK(BM_SharedMutexFair_CoroLockUnlock);

static void BM_HybridRenameMutex_CoroLockUnlock(benchmark::State& state) {
  folly::CPUThreadPoolExecutor exec(1);
  HybridRenameMutex m;
  folly::coro::blockingWait(
      folly::coro::co_invoke([&]() -> folly::coro::Task<void> {
        for (auto _ : state) {
          co_await m.co_lock();
          m.unlock();
        }
        co_return;
      }).scheduleOn(&exec));
}
BENCHMARK(BM_HybridRenameMutex_CoroLockUnlock);

// =====================================================================
// Cross-thread Token release — measures the SharedMutex Token fast
// path under cross-thread acquire/release (per
// xplat/folly/SharedMutex.h:183-193 — "It is explicitly allowed to
// call unlock_shared() from a different thread than lock_shared(),
// so long as they are properly paired.")
// =====================================================================

static void BM_SharedMutex_LockSharedUnlockShared_TokenCrossThread(
    benchmark::State& state) {
  folly::SharedMutex m;
  // Acquire on this thread; release on a worker thread; rendezvous via
  // futures. The rendezvous cost dominates here, so this benchmark is
  // most useful as a relative comparison vs the same-thread Token
  // benchmark above (to bound the cross-thread fast path overhead).
  folly::CPUThreadPoolExecutor exec(1);
  for (auto _ : state) {
    folly::SharedMutex::Token token;
    m.lock_shared(token);
    folly::via(&exec, [&m, t = token]() mutable { m.unlock_shared(t); }).get();
  }
}
BENCHMARK(BM_SharedMutex_LockSharedUnlockShared_TokenCrossThread);

// =====================================================================
// Contended readers under writer pressure
//
// Threaded benchmark: N readers + 1 writer (multiplexed onto the
// reader threads via a counter). Each iteration acquires shared, does a
// trivial computation, releases. Periodically a writer acquires
// exclusive and releases.
//
// Google Benchmark threading: state.threads gives N. We use a
// per-thread counter to designate one thread as the writer.
// =====================================================================

namespace bench_contended {

template <typename Mutex>
struct Fixture {
  Mutex mutex;
  std::atomic<uint64_t> writerCount{0};
};

// Static per-mutex-type fixtures (constructed once, shared across all
// thread invocations of the same benchmark family).
auto& sharedMutexFixture() {
  static Fixture<folly::SharedMutex> f;
  return f;
}
auto& sharedMutexFairFixture() {
  static Fixture<folly::coro::SharedMutexFair> f;
  return f;
}
auto& hybridFixture() {
  static Fixture<HybridRenameMutex> f;
  return f;
}

constexpr int kWriterPeriod = 100; // 1 writer acquire per 100 reader acquires

} // namespace bench_contended

static void BM_Contended_SharedMutex(benchmark::State& state) {
  auto& f = bench_contended::sharedMutexFixture();
  uint64_t localCount = 0;
  for (auto _ : state) {
    if ((localCount++ % bench_contended::kWriterPeriod) == 0 &&
        state.thread_index() == 0) {
      f.mutex.lock();
      f.writerCount.fetch_add(1, std::memory_order_relaxed);
      f.mutex.unlock();
    } else {
      f.mutex.lock_shared();
      benchmark::DoNotOptimize(f.writerCount.load(std::memory_order_relaxed));
      f.mutex.unlock_shared();
    }
  }
}
BENCHMARK(BM_Contended_SharedMutex)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8);

static void BM_Contended_SharedMutexFair_Sync(benchmark::State& state) {
  auto& f = bench_contended::sharedMutexFairFixture();
  uint64_t localCount = 0;
  for (auto _ : state) {
    if ((localCount++ % bench_contended::kWriterPeriod) == 0 &&
        state.thread_index() == 0) {
      folly::coro::blockingWait(f.mutex.co_scoped_lock());
      f.writerCount.fetch_add(1, std::memory_order_relaxed);
    } else {
      auto guard = folly::coro::blockingWait(f.mutex.co_scoped_lock_shared());
      benchmark::DoNotOptimize(f.writerCount.load(std::memory_order_relaxed));
    }
  }
}
BENCHMARK(BM_Contended_SharedMutexFair_Sync)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8);

static void BM_Contended_HybridRenameMutex(benchmark::State& state) {
  auto& f = bench_contended::hybridFixture();
  uint64_t localCount = 0;
  HybridRenameMutex::Token token;
  for (auto _ : state) {
    if ((localCount++ % bench_contended::kWriterPeriod) == 0 &&
        state.thread_index() == 0) {
      f.mutex.lock();
      f.writerCount.fetch_add(1, std::memory_order_relaxed);
      f.mutex.unlock();
    } else {
      f.mutex.lock_shared(token);
      benchmark::DoNotOptimize(f.writerCount.load(std::memory_order_relaxed));
      f.mutex.unlock_shared(token);
    }
  }
}
BENCHMARK(BM_Contended_HybridRenameMutex)
    ->Threads(1)
    ->Threads(2)
    ->Threads(4)
    ->Threads(8);

} // namespace

BENCHMARK_MAIN();
