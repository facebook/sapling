/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "Journal.h"
#include <folly/logging/xlog.h>

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

void Journal::truncateIfNecessary(
    folly::Synchronized<DeltaState>::LockedPtr& deltaState) {
  while (!deltaState->deltas.empty() &&
         deltaState->stats->memoryUsage > deltaState->memoryLimit) {
    deltaState->stats->entryCount--;
    deltaState->stats->memoryUsage -=
        deltaState->deltas.front().estimateMemoryUsage();
    deltaState->deltas.pop_front();
  }
}

void Journal::addDelta(std::unique_ptr<JournalDelta>&& delta) {
  {
    auto deltaState = deltaState_.wlock();

    delta->sequenceID = deltaState->nextSequence++;

    delta->time = std::chrono::steady_clock::now();

    // If the hashes were not set to anything, default to copying
    // the value from the prior journal entry
    if (!deltaState->deltas.empty() && delta->fromHash == kZeroHash &&
        delta->toHash == kZeroHash) {
      JournalDelta& previous = deltaState->deltas.back();
      delta->fromHash = previous.toHash;
      delta->toHash = delta->fromHash;
    }

    // Check memory before adding the new delta to make sure we always
    // have at least one delta (other than when the journal starts up)
    truncateIfNecessary(deltaState);

    // We will compact the delta if possible. We can compact the delta if it is
    // a modification to a single file and matches the last delta added to the
    // Journal. For a consumer the only differences seen due to compaction are
    // that:
    // - getDebugRawJournalInfo will skip entries in its list
    // - The stats should show a different memory usage and number of entries
    // - accumulateRange will return a different fromSequence and fromTime than
    // what would happen if the deltas were not compacted [e.g. JournalDelta 3
    // and 4 are the same modification, accumulateRange(3) would have a
    // fromSequence of 3 without compaction and a fromSequence of 4 with
    // compaction]
    if (!deltaState->deltas.empty() && delta->isModification() &&
        delta->isSameAction(deltaState->deltas.back())) {
      deltaState->stats->latestTimestamp = delta->time;
      deltaState->stats->memoryUsage -=
          deltaState->deltas.back().estimateMemoryUsage();
      deltaState->stats->memoryUsage += delta->estimateMemoryUsage();
      deltaState->deltas.back() = std::move(*delta);
    } else {
      if (deltaState->stats) {
        ++(deltaState->stats->entryCount);
        deltaState->stats->memoryUsage += delta->estimateMemoryUsage();
      } else {
        deltaState->stats = JournalStats();
        deltaState->stats->entryCount = 1;
        deltaState->stats->memoryUsage = delta->estimateMemoryUsage();
      }
      deltaState->stats->latestTimestamp = delta->time;
      deltaState->deltas.emplace_back(std::move(*delta));
    }

    deltaState->stats->earliestTimestamp = deltaState->deltas.front().time;
  }

  // Careful to call the subscribers with no locks held.
  auto subscribers = subscriberState_.rlock()->subscribers;
  for (auto& sub : subscribers) {
    sub.second();
  }
}

std::optional<JournalDeltaInfo> Journal::getLatest() const {
  auto deltaState = deltaState_.rlock();
  if (deltaState->deltas.empty()) {
    return std::nullopt;
  } else {
    const JournalDelta& back = deltaState->deltas.back();
    return JournalDeltaInfo{
        back.fromHash, back.toHash, back.sequenceID, back.time};
  }
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

namespace {
folly::StringPiece eventCharacterizationFor(const PathChangeInfo& ci) {
  if (ci.existedBefore && !ci.existedAfter) {
    return "Removed";
  } else if (!ci.existedBefore && ci.existedAfter) {
    return "Created";
  } else if (ci.existedBefore && ci.existedAfter) {
    return "Changed";
  } else {
    return "Ghost";
  }
}
} // namespace

void Journal::setMemoryLimit(size_t limit) {
  auto deltaState = deltaState_.wlock();
  deltaState->memoryLimit = limit;
}

size_t Journal::getMemoryLimit() const {
  auto deltaState = deltaState_.rlock();
  return deltaState->memoryLimit;
}

std::unique_ptr<JournalDeltaRange> Journal::accumulateRange() const {
  return accumulateRange(1);
}

std::unique_ptr<JournalDeltaRange> Journal::accumulateRange(
    SequenceNumber from) const {
  DCHECK(from > 0);
  std::unique_ptr<JournalDeltaRange> result = nullptr;

  auto deltaState = deltaState_.rlock();
  // If this is going to be truncated handle it before iterating.
  if (!deltaState->deltas.empty() &&
      deltaState->deltas.front().sequenceID > from) {
    result = std::make_unique<JournalDeltaRange>();
    result->isTruncated = true;
  } else {
    forEachDelta(
        deltaState->deltas,
        from,
        std::nullopt,
        [&result](const JournalDelta& current) -> void {
          if (!result) {
            result = std::make_unique<JournalDeltaRange>();
            result->toSequence = current.sequenceID;
            result->toTime = current.time;
            result->toHash = current.toHash;
          }
          // Capture the lower bound.
          result->fromSequence = current.sequenceID;
          result->fromTime = current.time;
          result->fromHash = current.fromHash;

          // Merge the unclean status list
          result->uncleanPaths.insert(
              current.uncleanPaths.begin(), current.uncleanPaths.end());

          for (auto& entry : current.getChangedFilesInOverlay()) {
            auto& name = entry.first;
            auto& currentInfo = entry.second;
            auto* resultInfo =
                folly::get_ptr(result->changedFilesInOverlay, name);
            if (!resultInfo) {
              result->changedFilesInOverlay.emplace(name, currentInfo);
            } else {
              if (resultInfo->existedBefore != currentInfo.existedAfter) {
                auto event1 = eventCharacterizationFor(currentInfo);
                auto event2 = eventCharacterizationFor(*resultInfo);
                XLOG(ERR) << "Journal for " << name << " holds invalid "
                          << event1 << ", " << event2 << " sequence";
              }

              resultInfo->existedBefore = currentInfo.existedBefore;
            }
          }
        });
  }

  if (result && result->isTruncated) {
    if (edenStats_) {
      edenStats_->getJournalStatsForCurrentThread().truncatedReads.addValue(1);
    }
  }

  return result;
}

std::vector<DebugJournalDelta> Journal::getDebugRawJournalInfo(
    SequenceNumber from,
    std::optional<size_t> limit,
    long mountGeneration) const {
  auto result = std::vector<DebugJournalDelta>();
  auto deltaState = deltaState_.rlock();
  forEachDelta(
      deltaState->deltas,
      from,
      limit,
      [mountGeneration, &result](const JournalDelta& current) -> void {
        DebugJournalDelta delta;
        JournalPosition fromPosition;
        fromPosition.set_mountGeneration(mountGeneration);
        fromPosition.set_sequenceNumber(current.sequenceID);
        fromPosition.set_snapshotHash(thriftHash(current.fromHash));
        delta.set_fromPosition(fromPosition);

        JournalPosition toPosition;
        toPosition.set_mountGeneration(mountGeneration);
        toPosition.set_sequenceNumber(current.sequenceID);
        toPosition.set_snapshotHash(thriftHash(current.toHash));
        delta.set_toPosition(toPosition);

        for (const auto& entry : current.getChangedFilesInOverlay()) {
          auto& path = entry.first;
          auto& changeInfo = entry.second;

          DebugPathChangeInfo debugChangeInfo;
          debugChangeInfo.existedBefore = changeInfo.existedBefore;
          debugChangeInfo.existedAfter = changeInfo.existedAfter;
          delta.changedPaths.emplace(path.stringPiece().str(), debugChangeInfo);
        }

        for (auto& path : current.uncleanPaths) {
          delta.uncleanPaths.emplace(path.stringPiece().str());
        }

        result.push_back(delta);
      });
  return result;
}

// Func: void(const JournalDelta&)
template <class Func>
void Journal::forEachDelta(
    const std::deque<JournalDelta>& deltas,
    SequenceNumber from,
    std::optional<size_t> lengthLimit,
    Func&& deltaCallback) const {
  size_t iters = 0;
  for (auto deltaIter = deltas.rbegin(); deltaIter != deltas.rend();
       ++deltaIter) {
    const JournalDelta& current = *deltaIter;
    if (current.sequenceID < from) {
      break;
    }
    if (lengthLimit && iters >= lengthLimit.value()) {
      break;
    }
    deltaCallback(current);
    ++iters;
  }
}
} // namespace eden
} // namespace facebook
