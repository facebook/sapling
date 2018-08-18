/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "Journal.h"

namespace facebook {
namespace eden {

void Journal::addDelta(std::unique_ptr<JournalDelta>&& delta) {
  {
    auto deltaState = deltaState_.wlock();

    delta->toSequence = deltaState->nextSequence++;
    delta->fromSequence = delta->toSequence;

    delta->toTime = std::chrono::steady_clock::now();
    delta->fromTime = delta->toTime;

    delta->previous = deltaState->latest;

    // If the hashes were not set to anything, default to copying
    // the value from the prior journal entry
    if (delta->previous && delta->fromHash == kZeroHash &&
        delta->toHash == kZeroHash) {
      delta->fromHash = delta->previous->toHash;
      delta->toHash = delta->fromHash;
    }

    deltaState->latest = JournalDeltaPtr{std::move(delta)};
  }

  // Careful to call the subscribers with no locks held.
  auto subscribers = subscriberState_.rlock()->subscribers;
  for (auto& sub : subscribers) {
    sub.second();
  }
}

JournalDeltaPtr Journal::getLatest() const {
  return deltaState_.rlock()->latest;
}

void Journal::replaceJournal(std::unique_ptr<JournalDelta>&& delta) {
  auto deltaState = deltaState_.wlock();
  deltaState->latest = JournalDeltaPtr{std::move(delta)};
}

uint64_t Journal::registerSubscriber(SubscriberCallback&& callback) {
  auto subscriberState = subscriberState_.wlock();
  auto id = subscriberState->nextSubscriberId++;
  subscriberState->subscribers[id] = std::move(callback);
  return id;
}

void Journal::cancelSubscriber(uint64_t id) {
  subscriberState_.wlock()->subscribers.erase(id);
}

void Journal::cancelAllSubscribers() {
  subscriberState_.wlock()->subscribers.clear();
}

bool Journal::isSubscriberValid(uint64_t id) const {
  auto subscriberState = subscriberState_.rlock();
  auto& subscribers = subscriberState->subscribers;
  return subscribers.find(id) != subscribers.end();
}

} // namespace eden
} // namespace facebook
