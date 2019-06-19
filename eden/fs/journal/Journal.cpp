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

void Journal::recordCreated(RelativePathPiece fileName) {
  addDelta(std::make_unique<JournalDelta>(fileName, JournalDelta::CREATED));
}

void Journal::recordRemoved(RelativePathPiece fileName) {
  addDelta(std::make_unique<JournalDelta>(fileName, JournalDelta::REMOVED));
}

void Journal::recordChanged(RelativePathPiece fileName) {
  addDelta(std::make_unique<JournalDelta>(fileName, JournalDelta::CHANGED));
}

void Journal::recordRenamed(
    RelativePathPiece oldName,
    RelativePathPiece newName) {
  addDelta(
      std::make_unique<JournalDelta>(oldName, newName, JournalDelta::RENAMED));
}

void Journal::recordReplaced(
    RelativePathPiece oldName,
    RelativePathPiece newName) {
  addDelta(
      std::make_unique<JournalDelta>(oldName, newName, JournalDelta::REPLACED));
}

void Journal::recordHashUpdate(Hash toHash) {
  auto delta = std::make_unique<JournalDelta>();
  delta->toHash = toHash;
  addDelta(std::move(delta));
}

void Journal::recordHashUpdate(Hash fromHash, Hash toHash) {
  auto delta = std::make_unique<JournalDelta>();
  delta->fromHash = fromHash;
  delta->toHash = toHash;
  addDelta(std::move(delta));
}

void Journal::recordUncleanPaths(
    Hash fromHash,
    Hash toHash,
    std::unordered_set<RelativePath>&& uncleanPaths) {
  auto delta = std::make_unique<JournalDelta>();
  delta->fromHash = fromHash;
  delta->toHash = toHash;
  delta->uncleanPaths = std::move(uncleanPaths);
  addDelta(std::move(delta));
}

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

    if (deltaState->stats) {
      ++(deltaState->stats->entryCount);
      deltaState->stats->memoryUsage += delta->estimateMemoryUsage();
      deltaState->stats->earliestTimestamp =
          std::min(deltaState->stats->earliestTimestamp, delta->fromTime);
      deltaState->stats->latestTimestamp =
          std::max(deltaState->stats->latestTimestamp, delta->toTime);
    } else {
      deltaState->stats = JournalStats();
      deltaState->stats->entryCount = 1;
      deltaState->stats->memoryUsage = delta->estimateMemoryUsage();
      deltaState->stats->earliestTimestamp = delta->fromTime;
      deltaState->stats->latestTimestamp = delta->toTime;
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
  return deltaState_.rlock()->stats;
}
} // namespace eden
} // namespace facebook
