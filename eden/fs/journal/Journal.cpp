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
  auto subscriberState = subscriberState_.wlock();
  auto it = subscriberState->subscribers.find(id);
  if (it == subscriberState->subscribers.end()) {
    return;
  }
  // Extend the lifetime of the value we're removing
  auto callback = std::move(it->second);
  subscriberState->subscribers.erase(it);
  // release the lock before we trigger the destructor
  subscriberState.unlock();
  // callback can now run its destructor outside the lock
}

void Journal::cancelAllSubscribers() {
  // Take care: some subscribers will attempt to call cancelSubscriber()
  // as part of their tear down, so we need to make sure that we aren't
  // holding the lock when we trigger that.
  std::unordered_map<SubscriberId, SubscriberCallback> subscribers;
  subscriberState_.wlock()->subscribers.swap(subscribers);
  subscribers.clear();
}

bool Journal::isSubscriberValid(uint64_t id) const {
  auto subscriberState = subscriberState_.rlock();
  auto& subscribers = subscriberState->subscribers;
  return subscribers.find(id) != subscribers.end();
}

std::optional<JournalStats> Journal::getStats() {
  JournalStats stats;
  auto curr = getLatest();
  if (curr == nullptr) {
    // return None since this is an empty journal
    return std::nullopt;
  }
  stats.latestTimestamp = curr->toTime;
  stats.earliestTimestamp = curr->fromTime;
  while (curr != nullptr) {
    ++stats.entryCount;
    stats.latestTimestamp = std::max(stats.latestTimestamp, curr->toTime);
    stats.earliestTimestamp = std::min(stats.earliestTimestamp, curr->fromTime);
    curr = curr->previous;
  }
  return stats;
}

} // namespace eden
} // namespace facebook
