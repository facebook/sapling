/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/InvalidationQueue.h"

#include <folly/CancellationToken.h>
#include <folly/ScopeGuard.h>
#include <gtest/gtest.h>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <mutex>
#include <set>
#include <thread>
#include <vector>

using namespace facebook::eden;

namespace {

constexpr auto kTimeout = std::chrono::seconds{10};

struct TestEntry {
  int id;
};

/**
 * Records the entries a queue processes, and holds the ones the test asked
 * for until it releases them, standing in for an invalidation blocked in the
 * kernel.
 */
class Recorder {
 public:
  InvalidationQueue<TestEntry>::Worker worker() {
    return [this](TestEntry& entry) {
      std::unique_lock lock{mutex_};
      started_.push_back(entry.id);
      cv_.notify_all();
      cv_.wait(lock, [&] { return !held_.count(entry.id); });
      processed_.push_back(entry.id);
      cv_.notify_all();
    };
  }

  void hold(int id) {
    std::lock_guard lock{mutex_};
    held_.insert(id);
  }

  void release(int id) {
    {
      std::lock_guard lock{mutex_};
      held_.erase(id);
    }
    cv_.notify_all();
  }

  bool waitUntilStarted(int id) {
    std::unique_lock lock{mutex_};
    return cv_.wait_for(lock, kTimeout, [&] {
      return std::find(started_.begin(), started_.end(), id) != started_.end();
    });
  }

  std::vector<int> processed() {
    std::lock_guard lock{mutex_};
    return processed_;
  }

 private:
  std::mutex mutex_;
  std::condition_variable cv_;
  std::set<int> held_;
  std::vector<int> started_;
  std::vector<int> processed_;
};

template <typename Pred>
bool waitFor(Pred pred) {
  auto deadline = std::chrono::steady_clock::now() + kTimeout;
  while (std::chrono::steady_clock::now() < deadline) {
    if (pred()) {
      return true;
    }
    std::this_thread::yield();
  }
  return pred();
}

} // namespace

TEST(InvalidationQueueTest, singleWorkerProcessesInOrderAndFlushes) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{1, recorder.worker(), "inval-test"};
  queue.start();

  EXPECT_TRUE(queue.add(TestEntry{1}));
  EXPECT_TRUE(queue.add(TestEntry{2}));
  EXPECT_TRUE(queue.add(TestEntry{3}));
  std::move(queue.flush()).get(kTimeout);

  EXPECT_EQ((std::vector<int>{1, 2, 3}), recorder.processed());
  EXPECT_FALSE(queue.flushInProgress());
}

TEST(InvalidationQueueTest, zeroWorkersStillProcessesEntries) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{0, recorder.worker(), "inval-test"};
  queue.start();
  EXPECT_TRUE(queue.add(TestEntry{1}));
  std::move(queue.flush()).get(kTimeout);
  EXPECT_EQ((std::vector<int>{1}), recorder.processed());
}

TEST(InvalidationQueueTest, multiWorkerFlushWaitsForInflightAndBlocksLater) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{2, recorder.worker(), "inval-test"};
  queue.start();
  recorder.hold(1);
  SCOPE_EXIT {
    recorder.release(1);
  };

  ASSERT_TRUE(queue.add(TestEntry{1}));
  ASSERT_TRUE(recorder.waitUntilStarted(1));

  // The flush reaches the front of the queue while entry 1 is in flight, so
  // it waits, and stops later entries from starting while it does.
  auto firstFlush = queue.flush();
  ASSERT_TRUE(waitFor([&] { return queue.flushInProgress(); }));
  ASSERT_TRUE(queue.add(TestEntry{2}));
  auto secondFlush = queue.flush();
  EXPECT_FALSE(firstFlush.isReady());
  EXPECT_FALSE(secondFlush.isReady());
  EXPECT_EQ(2u, queue.size());
  EXPECT_TRUE(recorder.processed().empty());

  recorder.release(1);
  std::move(firstFlush).get(kTimeout);
  std::move(secondFlush).get(kTimeout);
  EXPECT_EQ((std::vector<int>{1, 2}), recorder.processed());
}

TEST(InvalidationQueueTest, stopDrainsQueuedFlushesAndRejectsNewEntries) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{2, recorder.worker(), "inval-test"};
  queue.start();
  recorder.hold(1);
  std::thread stopper;
  SCOPE_EXIT {
    recorder.release(1);
    if (stopper.joinable()) {
      stopper.join();
    }
  };

  ASSERT_TRUE(queue.add(TestEntry{1}));
  ASSERT_TRUE(recorder.waitUntilStarted(1));
  auto firstFlush = queue.flush();
  ASSERT_TRUE(waitFor([&] { return queue.flushInProgress(); }));
  auto secondFlush = queue.flush();

  std::atomic<bool> stopFinished{false};
  stopper = std::thread([&] {
    queue.stop();
    stopFinished.store(true, std::memory_order_release);
  });
  ASSERT_TRUE(waitFor([&] {
    return queue.state() == InvalidationQueue<TestEntry>::State::DRAINING;
  }));
  EXPECT_FALSE(stopFinished.load(std::memory_order_acquire));

  // Draining: new entries are dropped, and a new flush completes at once.
  auto sizeBefore = queue.size();
  EXPECT_FALSE(queue.add(TestEntry{2}));
  EXPECT_EQ(sizeBefore, queue.size());
  std::move(queue.flush()).get(kTimeout);

  recorder.release(1);
  stopper.join();
  EXPECT_TRUE(stopFinished.load(std::memory_order_acquire));
  std::move(firstFlush).get(kTimeout);
  std::move(secondFlush).get(kTimeout);
  EXPECT_EQ(InvalidationQueue<TestEntry>::State::STOPPED, queue.state());
  EXPECT_EQ(0u, queue.size());
  EXPECT_EQ((std::vector<int>{1}), recorder.processed());
}

TEST(InvalidationQueueTest, concurrentStopsAreSerialized) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{2, recorder.worker(), "inval-test"};
  queue.start();

  std::atomic<size_t> ready{0};
  std::atomic<bool> go{false};
  auto stop = [&] {
    ready.fetch_add(1, std::memory_order_release);
    while (!go.load(std::memory_order_acquire)) {
      std::this_thread::yield();
    }
    queue.stop();
  };
  std::thread first{stop};
  std::thread second{stop};
  while (ready.load(std::memory_order_acquire) != 2) {
    std::this_thread::yield();
  }
  go.store(true, std::memory_order_release);
  first.join();
  second.join();

  EXPECT_EQ(InvalidationQueue<TestEntry>::State::STOPPED, queue.state());
}

TEST(InvalidationQueueTest, addWithLimitWaitsForCapacity) {
  constexpr size_t kLimit = 4;
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{1, recorder.worker(), "inval-test"};
  // Not started: the queue fills up.
  for (int i = 1; i <= static_cast<int>(kLimit); ++i) {
    ASSERT_TRUE(queue.add(TestEntry{i}));
  }

  folly::CancellationSource cancellation;
  bool waited = false;
  bool enqueued = false;
  std::thread producer([&] {
    enqueued = queue.addWithLimit(
        TestEntry{5}, kLimit, cancellation.getToken(), &waited);
  });
  SCOPE_EXIT {
    cancellation.requestCancellation();
    if (producer.joinable()) {
      producer.join();
    }
  };
  ASSERT_TRUE(waitFor([&] { return queue.numCapacityWaiters() == 1; }));

  // Once a worker drains the queue the producer gets in.
  queue.start();
  producer.join();
  EXPECT_TRUE(enqueued);
  EXPECT_TRUE(waited);
  std::move(queue.flush()).get(kTimeout);
  EXPECT_EQ((std::vector<int>{1, 2, 3, 4, 5}), recorder.processed());
  EXPECT_EQ(0u, queue.numCapacityWaiters());
}

TEST(InvalidationQueueTest, addWithLimitOfZeroRejects) {
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{1, recorder.worker(), "inval-test"};
  folly::CancellationSource cancellation;
  EXPECT_FALSE(queue.addWithLimit(TestEntry{1}, 0, cancellation.getToken()));
  EXPECT_EQ(0u, queue.size());
}

TEST(InvalidationQueueTest, addWithLimitIsCancellable) {
  constexpr size_t kLimit = 4;
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{1, recorder.worker(), "inval-test"};
  for (int i = 1; i <= static_cast<int>(kLimit); ++i) {
    ASSERT_TRUE(queue.add(TestEntry{i}));
  }

  folly::CancellationSource cancellation;
  bool enqueued = true;
  std::thread producer([&] {
    enqueued =
        queue.addWithLimit(TestEntry{5}, kLimit, cancellation.getToken());
  });
  SCOPE_EXIT {
    cancellation.requestCancellation();
    if (producer.joinable()) {
      producer.join();
    }
  };
  ASSERT_TRUE(waitFor([&] { return queue.numCapacityWaiters() == 1; }));

  cancellation.requestCancellation();
  producer.join();
  EXPECT_FALSE(enqueued);
  EXPECT_EQ(kLimit, queue.size());
}

TEST(InvalidationQueueTest, stopUnblocksWaitingProducer) {
  constexpr size_t kLimit = 4;
  Recorder recorder;
  InvalidationQueue<TestEntry> queue{1, recorder.worker(), "inval-test"};
  for (int i = 1; i <= static_cast<int>(kLimit); ++i) {
    ASSERT_TRUE(queue.add(TestEntry{i}));
  }

  folly::CancellationSource cancellation;
  bool enqueued = true;
  std::thread producer([&] {
    enqueued =
        queue.addWithLimit(TestEntry{5}, kLimit, cancellation.getToken());
  });
  SCOPE_EXIT {
    if (producer.joinable()) {
      producer.join();
    }
  };
  ASSERT_TRUE(waitFor([&] { return queue.numCapacityWaiters() == 1; }));

  // Stopping with no workers abandons the entries but wakes the producer.
  queue.stop();
  producer.join();
  EXPECT_FALSE(enqueued);
  EXPECT_EQ(InvalidationQueue<TestEntry>::State::STOPPED, queue.state());
  // A flush after stop completes at once; in debug builds ImmediateFuture
  // reports even a ready value as not ready, so wait rather than check.
  std::move(queue.flush()).get(kTimeout);
}
