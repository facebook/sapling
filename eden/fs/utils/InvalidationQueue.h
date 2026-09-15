/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/CancellationToken.h>
#include <folly/Function.h>
#include <folly/ScopeGuard.h>
#include <folly/Synchronized.h>
#include <folly/futures/Promise.h>
#include <folly/logging/xlog.h>
#include <folly/synchronization/CallOnce.h>
#include <folly/system/ThreadName.h>

#include <atomic>
#include <condition_variable>
#include <deque>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <utility>
#include <variant>
#include <vector>

#include "eden/common/utils/ImmediateFuture.h"

namespace facebook::eden {

/**
 * A queue of kernel cache invalidations processed by dedicated threads.
 * FuseChannel and Nfsd3 each own one.
 *
 * Invalidations run on their own threads because they may block in the kernel
 * until it can take a lock that another thread already holds while waiting on
 * one of EdenFS's own locks. For example, a process calling unlink(parent,
 * "foo") holds the kernel's inode lock on parent when the kernel sends the
 * unlink to EdenFS, which needs the mount's rename lock. A checkout holding
 * that rename lock generates invalidations; if it waited for them inline it
 * would deadlock. Producers must therefore never wait for an invalidation to
 * complete while holding EdenFS locks.
 *
 * flush() is a barrier: its future completes once every entry queued before
 * it has been processed. With one worker the queue is processed in order, in
 * batches. With several, workers take entries one at a time and complete them
 * in any order, and a flush waits for the entries in flight before letting
 * later ones start.
 *
 * addWithLimit() gives bulk producers such as inode GC backpressure: it
 * waits, cancellably, while the queue holds the given number of entries or
 * more. With one worker the worker takes the whole queue at once, so up to
 * twice the limit can be outstanding.
 *
 * stop() drains what is queued, keeps fulfilling flushes, rejects new entries,
 * and joins the workers.
 */
template <typename Entry>
class InvalidationQueue {
 public:
  enum class State : uint32_t {
    ACCEPTING,
    DRAINING,
    STOPPED,
  };

  /**
   * Processes one entry on a worker thread. It must not throw.
   */
  using Worker = folly::Function<void(Entry&)>;

  /**
   * The workers are not started until start() is called; entries queued
   * before that wait.
   */
  InvalidationQueue(size_t numWorkers, Worker worker, std::string threadName)
      : numWorkers_{numWorkers == 0 ? 1 : numWorkers},
        worker_{std::move(worker)},
        threadName_{std::move(threadName)} {
    if (numWorkers == 0) {
      XLOG(WARN, "Invalidation queue needs at least one worker; using one");
    }
  }

  ~InvalidationQueue() {
    stop();
  }

  InvalidationQueue(const InvalidationQueue&) = delete;
  InvalidationQueue& operator=(const InvalidationQueue&) = delete;
  InvalidationQueue(InvalidationQueue&&) = delete;
  InvalidationQueue& operator=(InvalidationQueue&&) = delete;

  size_t numWorkers() const {
    return numWorkers_;
  }

  /**
   * Spawn the worker threads.
   */
  void start() {
    for (size_t i = 0; i < numWorkers_; ++i) {
      threads_.emplace_back([this] { workerLoop(); });
    }
  }

  /**
   * Queue an entry. Returns false, dropping the entry, once stop() has begun.
   */
  bool add(Entry entry) {
    {
      auto queue = queue_.lock();
      if (queue->state != State::ACCEPTING) {
        return false;
      }
      queue->items.emplace_back(std::move(entry));
    }
    cv_.notify_one();
    return true;
  }

  /**
   * Queue several entries, taking the queue lock once.
   */
  template <typename Iter>
  void addAll(Iter begin, Iter end) {
    if (begin == end) {
      return;
    }
    {
      auto queue = queue_.lock();
      if (queue->state != State::ACCEPTING) {
        return;
      }
      for (; begin != end; ++begin) {
        queue->items.emplace_back(Entry{*begin});
      }
    }
    cv_.notify_all();
  }

  /**
   * Queue an entry once the queue holds fewer than maxQueueSize entries,
   * waiting for the workers if it does not. Returns false without queuing
   * when maxQueueSize is 0, when cancellation is requested before the entry
   * could be queued, or once stop() has begun. `waited` reports whether the
   * producer had to wait.
   */
  bool addWithLimit(
      Entry entry,
      size_t maxQueueSize,
      const folly::CancellationToken& cancellationToken,
      bool* waited = nullptr) {
    if (maxQueueSize == 0) {
      return false;
    }
    // Synchronize with the wait predicate so cancellation cannot notify
    // immediately before the producer starts waiting.
    auto wakeWaiters = [this] {
      auto queue = queue_.lock();
      queue.unlock();
      capacityCv_.notify_all();
    };
    folly::CancellationCallback cancellationCallback{
        cancellationToken, wakeWaiters};
    auto queue = queue_.lock();
    if (queue->items.size() >= maxQueueSize &&
        queue->state == State::ACCEPTING &&
        !cancellationToken.isCancellationRequested()) {
      if (waited) {
        *waited = true;
      }
      capacityWaiters_.fetch_add(1, std::memory_order_relaxed);
      SCOPE_EXIT {
        capacityWaiters_.fetch_sub(1, std::memory_order_relaxed);
      };
      capacityCv_.wait(queue.as_lock(), [&] {
        return queue->items.size() < maxQueueSize ||
            queue->state != State::ACCEPTING ||
            cancellationToken.isCancellationRequested();
      });
    }
    if (queue->state != State::ACCEPTING ||
        cancellationToken.isCancellationRequested()) {
      return false;
    }
    queue->items.emplace_back(std::move(entry));
    queue.unlock();
    cv_.notify_one();
    return true;
  }

  /**
   * Returns a future that completes once every entry queued before this call
   * has been processed. Completes immediately once stop() has begun: the
   * caller's mount is going away and nothing is left to wait for.
   */
  ImmediateFuture<folly::Unit> flush() {
    folly::Promise<folly::Unit> promise;
    auto result = promise.getFuture();
    {
      auto queue = queue_.lock();
      if (queue->state != State::ACCEPTING) {
        return folly::unit;
      }
      queue->items.emplace_back(Flush{std::move(promise)});
    }
    cv_.notify_one();
    return result;
  }

  /**
   * Stop accepting entries, let the workers drain what is queued, and join
   * them. Flushes still queued when the workers are gone are fulfilled.
   * Safe to call more than once and from several threads.
   */
  void stop() {
    folly::call_once(stopFlag_, [this] {
      {
        auto queue = queue_.lock();
        if (queue->state == State::ACCEPTING) {
          queue->state = State::DRAINING;
          if (!queue->items.empty()) {
            XLOGF(
                INFO,
                "draining {} pending invalidation(s) for {} before stopping invalidation workers",
                queue->items.size(),
                threadName_);
          }
          queue->items.emplace_back(Stop{});
        }
      }
      cv_.notify_all();
      capacityCv_.notify_all();

      for (auto& thread : threads_) {
        thread.join();
      }
      threads_.clear();

      std::deque<Item> abandoned;
      {
        auto queue = queue_.lock();
        if (queue->state != State::STOPPED) {
          queue->items.swap(abandoned);
          queue->flushInProgress = false;
          queue->state = State::STOPPED;
        }
      }
      for (auto& item : abandoned) {
        if (auto* flush = std::get_if<Flush>(&item)) {
          flush->promise.setValue();
        }
      }
      capacityCv_.notify_all();
    });
  }

  // Observers for tests.

  State state() const {
    return queue_.lock()->state;
  }

  size_t size() const {
    return queue_.lock()->items.size();
  }

  bool flushInProgress() const {
    return queue_.lock()->flushInProgress;
  }

  size_t numCapacityWaiters() const {
    return capacityWaiters_.load(std::memory_order_relaxed);
  }

 private:
  struct Flush {
    folly::Promise<folly::Unit> promise;
  };
  struct Stop {};
  using Item = std::variant<Entry, Flush, Stop>;

  struct Queue {
    std::deque<Item> items;
    bool flushInProgress{false};
    State state{State::ACCEPTING};
  };

  void process(Item& item) {
    if (auto* entry = std::get_if<Entry>(&item)) {
      worker_(*entry);
    } else if (auto* flush = std::get_if<Flush>(&item)) {
      // Everything queued before the flush has been processed.
      flush->promise.setValue();
    }
  }

  void notifyCapacityWaiters() {
    // Waiters increment the count under the queue lock before waiting, so a
    // blocked waiter is always visible to a worker that dequeued under the
    // same lock; skipping the broadcast when the count is zero cannot lose a
    // wakeup.
    if (capacityWaiters_.load(std::memory_order_relaxed) != 0) {
      capacityCv_.notify_all();
    }
  }

  void waitForInflight() {
    std::unique_lock lock{inflightMutex_};
    inflightCv_.wait(
        lock, [&] { return inflight_.load(std::memory_order_acquire) == 0; });
  }

  void markStopped() {
    {
      auto queue = queue_.lock();
      queue->state = State::STOPPED;
    }
    cv_.notify_all();
  }

  void workerLoop() noexcept {
    folly::setThreadName(threadName_);
    if (numWorkers_ == 1) {
      singleWorkerLoop();
    } else {
      multiWorkerLoop();
    }
  }

  /**
   * One worker takes the whole queue at a time and processes it in order, so
   * flushes need no in-flight accounting.
   */
  void singleWorkerLoop() noexcept {
    while (true) {
      std::deque<Item> items;
      {
        auto queue = queue_.lock();
        cv_.wait(queue.as_lock(), [&] {
          return queue->state == State::STOPPED || !queue->items.empty();
        });
        if (queue->state == State::STOPPED) {
          return;
        }
        queue->items.swap(items);
        notifyCapacityWaiters();
      }

      for (auto& item : items) {
        if (std::holds_alternative<Stop>(item)) {
          markStopped();
          return;
        }
        process(item);
      }
    }
  }

  /**
   * Several workers take one entry each and complete them concurrently.
   */
  void multiWorkerLoop() noexcept {
    while (true) {
      std::optional<Item> item;
      {
        auto queue = queue_.lock();
        cv_.wait(queue.as_lock(), [&] {
          return queue->state == State::STOPPED ||
              (!queue->items.empty() && !queue->flushInProgress);
        });
        if (queue->state == State::STOPPED) {
          return;
        }
        item.emplace(std::move(queue->items.front()));
        queue->items.pop_front();
        notifyCapacityWaiters();

        if (std::holds_alternative<Stop>(*item)) {
          queue.unlock();
          waitForInflight();
          markStopped();
          return;
        }

        if (std::holds_alternative<Flush>(*item)) {
          // A flush is a barrier: by the time it reaches the front of the
          // queue, all prior entries have either completed or are counted as
          // in flight. Keep later entries from starting while waiting so
          // later traffic cannot keep the in-flight count non-zero
          // indefinitely.
          queue->flushInProgress = true;
          queue.unlock();
          waitForInflight();
          {
            auto relock = queue_.lock();
            relock->flushInProgress = false;
          }
          cv_.notify_all();
          process(*item);
          continue;
        }

        // Count the entry as in flight before releasing the queue lock, so a
        // flush cannot slip between dequeue and execution and resolve before
        // this entry finishes.
        inflight_.fetch_add(1, std::memory_order_acq_rel);
      }

      SCOPE_EXIT {
        if (inflight_.fetch_sub(1, std::memory_order_acq_rel) == 1) {
          // Last in-flight entry finished: wake any flush waiter.
          std::lock_guard lock{inflightMutex_};
          inflightCv_.notify_all();
        }
      };
      process(*item);
    }
  }

  const size_t numWorkers_;
  Worker worker_;
  const std::string threadName_;

  folly::Synchronized<Queue, std::mutex> queue_;
  std::condition_variable cv_;
  std::condition_variable capacityCv_;
  std::atomic<size_t> capacityWaiters_{0};
  std::vector<std::thread> threads_;
  folly::once_flag stopFlag_;

  // Entries being processed by the multi-worker loop. A separate mutex from
  // queue_ so a waiting flush does not keep other workers from taking
  // entries.
  std::atomic<uint64_t> inflight_{0};
  std::mutex inflightMutex_;
  std::condition_variable inflightCv_;
};

} // namespace facebook::eden
