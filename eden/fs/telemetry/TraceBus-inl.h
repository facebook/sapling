/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>

namespace facebook::eden {

template <typename TraceEvent>
std::shared_ptr<TraceBus<TraceEvent>> TraceBus<TraceEvent>::create(
    std::string name,
    size_t bufferCapacity) {
  return std::make_shared<TraceBus<TraceEvent>>(
      PrivateConstructorTag{}, std::move(name), bufferCapacity);
}

template <typename TraceEvent>
TraceBus<TraceEvent>::TraceBus(
    PrivateConstructorTag,
    std::string name,
    size_t bufferCapacity)
    : name_{std::move(name)}, bufferCapacity_{bufferCapacity} {
  XCHECK_GT(bufferCapacity_, 0u) << "Buffer capacity must not be zero";

  state_.unsafeGetUnlocked().writeBuffer.reserve(bufferCapacity_);

  // Allocate the backbuffer here rather than in the thread so std::bad_alloc
  // can be caught.
  std::vector<TraceEvent> readBuffer;
  readBuffer.reserve(bufferCapacity);

  std::string threadName = "tracebus-" + name_;

  thread_ = std::thread{[this,
                         threadName = std::move(threadName),
                         readBuffer = std::move(readBuffer)]() mutable {
    folly::setThreadName(threadName);
    threadLoop(readBuffer);
  }};
}

template <typename TraceEvent>
TraceBus<TraceEvent>::~TraceBus() {
  state_.lock()->done = true;
  emptyCV_.notify_one();
  thread_.join();

  auto& state = state_.unsafeGetUnlocked();
  auto* p = state.subscriptions;
  while (p) {
    auto* next = p->next;
    delete p;
    p = next;
  }
}

template <typename TraceEvent>
void TraceBus<TraceEvent>::publish(const TraceEvent& event) noexcept {
  static_assert(std::is_nothrow_copy_constructible_v<TraceEvent>);
  publish(TraceEvent{event});
}

template <typename TraceEvent>
void TraceBus<TraceEvent>::publish(TraceEvent&& event) noexcept {
  static_assert(std::is_nothrow_move_constructible_v<TraceEvent>);

  bool wake;
  {
    auto state = state_.lock();
    XCHECK(!state->done) << "Illegal to publish concurrently with destruction";
    if (state->writeBuffer.size() == bufferCapacity_) {
      // If the buffer is full then the capacity is potentially set too low. Log
      // an appropriate warning and then block until we have room to append the
      // current event.
      logFullOnce();
      fullCV_.wait(state.as_lock(), [&] {
        return state->writeBuffer.size() < bufferCapacity_;
      });
    }
    wake = state->writeBuffer.empty();
    state->writeBuffer.push_back(std::move(event));
    state->sequenceNumber++;
  }
  if (wake) {
    emptyCV_.notify_one();
  }
}

template <typename TraceEvent>
TraceSubscriptionHandle<TraceEvent> TraceBus<TraceEvent>::subscribe(
    std::shared_ptr<Subscriber> subscriber) {
  auto* sub = new Subscription{std::move(subscriber)};
  // noexcept:
  auto state = state_.lock();
  sub->next = state->subscriptions;
  state->subscriptions = sub;
  hasSubscription_.store(true, std::memory_order_release);

  return SubscriptionHandle{sub, this->weak_from_this()};
}

template <typename TraceEvent>
void TraceBus<TraceEvent>::unsubscribe(void* subscription) noexcept {
  auto* sub = static_cast<Subscription*>(subscription);

  auto state = state_.lock();
  // Signal to threadLoop that `sub` should be deleted.
  sub->unsubscribe = state->sequenceNumber;

  // At this point, the memory referenced by `sub` must not be accessed as it
  // may be deleted at any moment.
}

template <typename TraceEvent>
void TraceBus<TraceEvent>::logFullOnce() noexcept {
  folly::call_once(logIfFullFlag_, [&]() noexcept {
    try {
      XLOG(WARN) << "TraceBus(" << name_ << ") is full; blocking. Is capacity "
                 << bufferCapacity_ << " sufficient?";
    } catch (std::exception& e) {
      fprintf(
          stderr,
          "TraceBus(%s) is full; blocking. Is capacity %" PRIu64
          "sufficient?\n"
          "Logging failed with %s\n",
          name_.c_str(),
          uint64_t{bufferCapacity_},
          e.what());
      fflush(stderr);
    }
  });
}

template <typename TraceEvent>
void TraceBus<TraceEvent>::threadLoop(
    std::vector<TraceEvent>& readBuffer) noexcept {
  // This function does no allocation and throws no exceptions.

  bool done = false;
  uint64_t lastObservedSequenceNumber = 0;
  while (!done) {
    XCHECK(readBuffer.empty())
        << "Avoid waiting while holding references to things";

    Subscription* head;
    {
      auto state = state_.lock();

      // Deallocate before we wait.
      // While the lock is held, delete all unsubscribed subscriptions.
      // plink is pointer to current node pointer.
      // nlink is pointer to next node pointer.
      // p is pointer to current node.
      Subscription** plink = &state->subscriptions;
      Subscription* p = *plink;
      while (p) {
        Subscription** nlink = &p->next;
        Subscription* next = *nlink;
        if (p->unsubscribe && p->unsubscribe <= lastObservedSequenceNumber) {
          // Here, we know this subscription has seen events up to (and possibly
          // beyond) its unsubscription request, so unlink it.
          *plink = *nlink;
          delete p;
        } else {
          // Otherwise, if the subscription has requested unsubscription, then
          // it needs to make one more iteration through the loop and will be
          // deleted after.
          plink = nlink;
        }
        p = next;
      }

      // TODO: If it were safe to access Subscription::unsubscribe when the lock
      // weren't held, it would be possible to check the unsubscribe sequence
      // number in the event iteration loop below and short-circuit observation
      // of events published after unsubscription.
      //
      // This probably isn't important.
      lastObservedSequenceNumber = state->sequenceNumber;

      if (state->subscriptions == nullptr) {
        hasSubscription_.store(false, std::memory_order_release);
      }

      // If no events are buffered, sleep until events are delivered or we are
      // signaled to terminate.
      emptyCV_.wait(state.as_lock(), [&] {
        return state->done || !state->writeBuffer.empty();
      });
      std::swap(state->writeBuffer, readBuffer);
      done = state->done;

      head = state->subscriptions;
    }

    // If the publish buffer filled, it's possible a publisher is waiting for
    // space, so wake them.
    if (readBuffer.size() == bufferCapacity_) {
      fullCV_.notify_all();
    }

    for (auto* sub = head; sub; sub = sub->next) {
      if (sub->hasThrownException) {
        continue;
      }
      const TraceEvent* begin = readBuffer.data();
      const TraceEvent* end = begin + readBuffer.size();
      try {
        sub->subscriber->observeBatch(begin, end);
      } catch (const std::exception& e) {
        sub->hasThrownException = true;
        XLOG(ERR) << "Subscription: " << sub->subscriber->name() << " threw "
                  << e.what() << ", unsubscribing.";
      }
    }

    readBuffer.clear();
  }
}

} // namespace facebook::eden
