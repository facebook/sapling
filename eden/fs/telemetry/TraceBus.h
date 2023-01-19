/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/synchronization/CallOnce.h>
#include <stdint.h>
#include <chrono>
#include <memory>
#include <thread>
#include <utility>

namespace facebook::eden {

/**
 * An optional base class for trace events that provides timestamps from when
 * the trace event was constructed.
 */
struct TraceEventBase {
  std::chrono::system_clock::time_point systemTime =
      std::chrono::system_clock::now();
  std::chrono::steady_clock::time_point monotonicTime =
      std::chrono::steady_clock::now();
};

template <typename TraceEvent>
class TraceBus;

/**
 * Base class for subscribers.
 */
template <typename TraceEvent>
class TraceEventSubscriber {
 public:
  /**
   * The name is used for logging error messages and need not be globally
   * unique.
   */
  explicit TraceEventSubscriber(std::string name) : name_{std::move(name)} {}
  virtual ~TraceEventSubscriber() = default;

  const std::string& name() const noexcept {
    return name_;
  }

  /**
   * Called on the TraceBus's background thread with a batch of published
   * events. Avoid blocking operations or operations that require heavy CPU
   * usage, as there is only one background thread per TraceBus, and it can back
   * up.
   */
  virtual void observeBatch(const TraceEvent* begin, const TraceEvent* end) = 0;

 private:
  const std::string name_;
};

/**
 * Subscriber class that calls a function object, used by
 * `TraceBus::subscribeFunction`.
 */
template <typename Fn, typename TraceEvent>
class FnTraceEventSubscriber final : public TraceEventSubscriber<TraceEvent> {
  using Base = TraceEventSubscriber<TraceEvent>;

 public:
  explicit FnTraceEventSubscriber(std::string name, Fn&& fn)
      : Base{std::move(name)}, fn_{std::move(fn)} {}

  void observeBatch(const TraceEvent* begin, const TraceEvent* end) override {
    for (const auto* p = begin; p != end; ++p) {
      fn_(*p);
    }
  }

 private:
  Fn fn_;
};

/**
 * Move-only handle that represents interest in a subscription. Unsubscribes
 * upon destruction or explicit `reset`.
 */
template <typename TraceEvent>
class TraceSubscriptionHandle {
 public:
  TraceSubscriptionHandle() = default;

  ~TraceSubscriptionHandle() {
    unsubscribe();
  }

  TraceSubscriptionHandle(TraceSubscriptionHandle&& that) noexcept
      : subscription_{std::exchange(that.subscription_, nullptr)},
        bus_{std::move(that.bus_)} {}

  TraceSubscriptionHandle& operator=(TraceSubscriptionHandle&& that) noexcept {
    unsubscribe();
    subscription_ = std::exchange(that.subscription_, nullptr);
    bus_ = std::move(that.bus_);
    return *this;
  }

  void reset() noexcept {
    unsubscribe();
    subscription_ = nullptr;
    bus_.reset();
  }

 private:
  explicit TraceSubscriptionHandle(
      void* subscription,
      std::weak_ptr<TraceBus<TraceEvent>> bus)
      : subscription_{subscription}, bus_{std::move(bus)} {}

  void unsubscribe() noexcept {
    if (subscription_) {
      if (auto bus = bus_.lock()) {
        bus->unsubscribe(subscription_);
      }
    }
    // No need to clear fields, because the caller will clobber them.
  }

  TraceSubscriptionHandle(const TraceSubscriptionHandle&) = delete;
  TraceSubscriptionHandle& operator=(const TraceSubscriptionHandle&) = delete;

  void* subscription_ = nullptr;
  std::weak_ptr<TraceBus<TraceEvent>> bus_;

  friend TraceBus<TraceEvent>;
};

/**
 * TraceBus is a reliable, fixed-capacity event trace that runs subscription
 * callbacks on a background thread. It is intended for lightweight telemetry
 * computation: if the subscriptions perform heavy computation and events are
 * submitted more frequently than they're processed, publish() will block.
 *
 * Note: this blocking behavior then waits for subscribers to finish processing
 * events, and if any locks are held that are subsequently attempted to be
 * acquired by a tracebus subscriber, this can cause a deadlock. As a general
 * rule one should try to avoid publishing to tracebus while holding any locks
 * and should be very careful when subscribers attempt to acquire locks.
 *
 * The capacity should be selected based on the expected usage in context.
 * Memory usage will be capacity * sizeof(TraceEvent) * 2, but a capacity too
 * small will block publishers. The buffer is not intended to prevent all
 * publishers from blocking, but to absorb latency in the case that subscribers
 * briefly cannot keep up.
 *
 * Ideally, capacity would be dynamically determined with algorithms similar to
 * network protocols, but a small fixed-size buffer should be sufficient.
 */
template <typename TraceEvent>
class TraceBus : public std::enable_shared_from_this<TraceBus<TraceEvent>> {
  struct PrivateConstructorTag {};

 public:
  using Subscriber = TraceEventSubscriber<TraceEvent>;
  using SubscriptionHandle = TraceSubscriptionHandle<TraceEvent>;

  /**
   * Creates a TraceBus. Returns a shared_ptr because the implementation relies
   * on weak_ptr, but in reality the strong reference count will stay at one,
   * unless the caller copies the shared_ptr.
   *
   * bufferCapacity must be nonzero.
   */
  static std::shared_ptr<TraceBus> create(
      std::string name,
      size_t bufferCapacity);

  /**
   * Use `create` instead. TraceBus must be managed by shared_ptr.
   */
  TraceBus(
      PrivateConstructorTag,
      std::string threadName,
      size_t bufferCapacity);

  /**
   * Blocks until all published events have been observed by all registered
   * subscribers.
   */
  ~TraceBus();

  /**
   * Publishes an event into the trace queue. The copy constructor must not
   * throw. Also, one should avoid publishing to tracebus while holding any
   * locks or ensure held locks are not attempted to be acquired by tracebus
   * subscribers. Otherwise, the thread could deadlock if capacity is reached
   */
  void publish(const TraceEvent& event) noexcept;

  /**
   * Publishes an event into the trace queue. The move constructor must not
   * throw. Also, one should avoid publishing to tracebus while holding any
   * locks or ensure held locks are not attempted to be acquired by tracebus
   * subscribers. Otherwise, the thread could deadlock if capacity is reached
   */
  void publish(TraceEvent&& event) noexcept;

  /**
   * Subscribe to published events. If the subscriber throws, it will
   * automatically be unsubscribed.
   *
   * Events are always observed by the order in which they're published, but
   * observers are not in any particular order relative to each other.
   *
   * The subscription will be unsubscribed when the returned handle is dropped.
   *
   * IMPORTANT: Even after a subscription handle is dropped, the callback may be
   * called a few more times, since the callback itself is not deleted until the
   * background thread gets to that. If using closures, be careful when
   * capturing raw pointers like `this`.
   */
  FOLLY_NODISCARD SubscriptionHandle
  subscribe(std::shared_ptr<Subscriber> subscriber);

  /**
   * Convenient `subscribe` wrapper that registers a function object.
   */
  template <typename Fn>
  FOLLY_NODISCARD SubscriptionHandle
  subscribeFunction(std::string name, Fn&& fn) {
    return subscribe(std::make_shared<FnTraceEventSubscriber<Fn, TraceEvent>>(
        std::move(name), std::forward<Fn>(fn)));
  };

  /**
   * A cheap check on if there is any subscription active for this TraceBus.
   * This method is prone to racy by nature (TOCTOU) and it is the best
   * approximation to detect if there is currently a subscriber active. New
   * subscriber may be added or removed after this function returns. Use with
   * caution.
   */
  bool hasSubscription() const {
    return hasSubscription_.load(std::memory_order_acquire);
  }

  TraceBus(TraceBus&&) = delete;
  TraceBus(const TraceBus&) = delete;
  TraceBus& operator=(TraceBus&&) = delete;
  TraceBus& operator=(const TraceBus&) = delete;

 private:
  /**
   * Remove a subscription. Does not block: the corresponding subscriber may
   * still be called with pending events by the background thread, but it is
   * guaranteed the subscriber will not see any events published after
   * unsubscribe() returns.
   */
  void unsubscribe(void* subscription) noexcept;

  void logFullOnce() noexcept;

  void threadLoop(std::vector<TraceEvent>& readbuffer) noexcept;

  struct Subscription {
    const std::shared_ptr<Subscriber> subscriber;

    // Accessed only on background thread. Set if the subscriber throws.
    bool hasThrownException = false;

    // If nonzero, unsubscription has been requested after the corresponding
    // sequenceNumber events have been observed. Only written or read while the
    // lock is held.
    uint64_t unsubscribe = 0;

    // Subscriptions form a linked list. Subscriptions insert to the head of the
    // list, and only while the lock is held. `threadLoop` is responsible for
    // deleting subscriptions. Once the head is read (while the lock is held),
    // `threadLoop` may traverse the list without the lock held, because it is
    // guaranteed nobody else will modify the `next` pointers.
    Subscription* next = nullptr;
  };

  struct State {
    bool done = false;
    Subscription* subscriptions = nullptr;
    std::vector<TraceEvent> writeBuffer;
    // Incremented every publish()
    uint64_t sequenceNumber = 1;
  };

  const std::string name_;
  const size_t bufferCapacity_;

  folly::Synchronized<State, std::mutex> state_;
  std::atomic_bool hasSubscription_{false};
  // Encodes the condition done || !writeBuffer.empty()
  std::condition_variable emptyCV_;
  // Encodes the condition writeBuffer.size() < bufferCapacity_
  std::condition_variable fullCV_;
  folly::once_flag logIfFullFlag_;
  std::thread thread_;

  // For unsubscribe.
  friend TraceSubscriptionHandle<TraceEvent>;
};

} // namespace facebook::eden

#include "eden/fs/telemetry/TraceBus-inl.h"
