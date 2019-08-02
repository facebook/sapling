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
  addDelta(FileChangeJournalDelta(fileName, FileChangeJournalDelta::CREATED));
}

void Journal::recordRemoved(RelativePathPiece fileName) {
  addDelta(FileChangeJournalDelta(fileName, FileChangeJournalDelta::REMOVED));
}

void Journal::recordChanged(RelativePathPiece fileName) {
  addDelta(FileChangeJournalDelta(fileName, FileChangeJournalDelta::CHANGED));
}

void Journal::recordRenamed(
    RelativePathPiece oldName,
    RelativePathPiece newName) {
  addDelta(FileChangeJournalDelta(
      oldName, newName, FileChangeJournalDelta::RENAMED));
}

void Journal::recordReplaced(
    RelativePathPiece oldName,
    RelativePathPiece newName) {
  addDelta(FileChangeJournalDelta(
      oldName, newName, FileChangeJournalDelta::REPLACED));
}

void Journal::recordHashUpdate(Hash toHash) {
  addDelta(HashUpdateJournalDelta{}, toHash);
}

void Journal::recordHashUpdate(Hash fromHash, Hash toHash) {
  HashUpdateJournalDelta delta;
  delta.fromHash = fromHash;
  addDelta(std::move(delta), toHash);
}

void Journal::recordUncleanPaths(
    Hash fromHash,
    Hash toHash,
    std::unordered_set<RelativePath>&& uncleanPaths) {
  HashUpdateJournalDelta delta;
  delta.fromHash = fromHash;
  delta.uncleanPaths = std::move(uncleanPaths);
  addDelta(std::move(delta), toHash);
}

JournalDeltaPtr Journal::DeltaState::frontPtr() noexcept {
  bool isFileChangeEmpty = fileChangeDeltas.empty();
  bool isHashUpdateEmpty = hashUpdateDeltas.empty();
  if (!isFileChangeEmpty && !isHashUpdateEmpty) {
    if (fileChangeDeltas.front().sequenceID <
        hashUpdateDeltas.front().sequenceID) {
      return &fileChangeDeltas.front();
    } else {
      return &hashUpdateDeltas.front();
    }
  }
  if (!isFileChangeEmpty) {
    return &fileChangeDeltas.front();
  } else if (!isHashUpdateEmpty) {
    return &hashUpdateDeltas.front();
  } else {
    return nullptr;
  }
}

void Journal::DeltaState::popFront() {
  bool isFileChangeEmpty = fileChangeDeltas.empty();
  bool isHashUpdateEmpty = hashUpdateDeltas.empty();
  if (!isFileChangeEmpty && !isHashUpdateEmpty) {
    if (fileChangeDeltas.front().sequenceID <
        hashUpdateDeltas.front().sequenceID) {
      fileChangeDeltas.pop_front();
    } else {
      hashUpdateDeltas.pop_front();
    }
  } else if (!isFileChangeEmpty) {
    fileChangeDeltas.pop_front();
  } else if (!isHashUpdateEmpty) {
    hashUpdateDeltas.pop_front();
  }
}

JournalDeltaPtr Journal::DeltaState::backPtr() noexcept {
  bool isFileChangeEmpty = fileChangeDeltas.empty();
  bool isHashUpdateEmpty = hashUpdateDeltas.empty();
  if (!isFileChangeEmpty && !isHashUpdateEmpty) {
    if (fileChangeDeltas.back().sequenceID >
        hashUpdateDeltas.back().sequenceID) {
      return &fileChangeDeltas.back();
    } else {
      return &hashUpdateDeltas.back();
    }
  }
  if (!isFileChangeEmpty) {
    return &fileChangeDeltas.back();
  } else if (!isHashUpdateEmpty) {
    return &hashUpdateDeltas.back();
  } else {
    return nullptr;
  }
}

void Journal::truncateIfNecessary(DeltaState& deltaState) {
  while (JournalDeltaPtr front = deltaState.frontPtr()) {
    if (estimateMemoryUsage(deltaState) <= deltaState.memoryLimit) {
      break;
    }
    deltaState.stats->entryCount--;

    deltaState.deltaMemoryUsage -= front.estimateMemoryUsage();
    deltaState.popFront();
  }
}

bool Journal::compact(FileChangeJournalDelta& delta, DeltaState& deltaState) {
  auto back = deltaState.backPtr().getAsFileChangeJournalDelta();
  if (back && delta.isModification() && delta.isSameAction(*back)) {
    deltaState.stats->latestTimestamp = delta.time;
    deltaState.deltaMemoryUsage -= back->estimateMemoryUsage();
    deltaState.deltaMemoryUsage += delta.estimateMemoryUsage();
    *back = std::move(delta);
    return true;
  }
  return false;
}

bool Journal::compact(
    HashUpdateJournalDelta& /* unused */,
    DeltaState& /* unused */) {
  return false;
}

template <typename T>
void Journal::addDeltaWithoutNotifying(T&& delta, DeltaState& deltaState) {
  delta.sequenceID = deltaState.nextSequence++;
  delta.time = std::chrono::steady_clock::now();

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
  if (!compact(delta, deltaState)) {
    if (deltaState.stats) {
      ++(deltaState.stats->entryCount);
      deltaState.deltaMemoryUsage += delta.estimateMemoryUsage();
    } else {
      deltaState.stats = JournalStats();
      deltaState.stats->entryCount = 1;
      deltaState.deltaMemoryUsage = delta.estimateMemoryUsage();
    }
    deltaState.stats->latestTimestamp = delta.time;
    deltaState.appendDelta(std::forward<T>(delta));
  }

  deltaState.stats->earliestTimestamp = deltaState.frontPtr()->time;
}

void Journal::notifySubscribers() const {
  auto subscribers = subscriberState_.rlock()->subscribers;
  for (auto& sub : subscribers) {
    sub.second();
  }
}

void Journal::addDelta(FileChangeJournalDelta&& delta) {
  {
    auto deltaState = deltaState_.wlock();
    addDeltaWithoutNotifying(std::move(delta), *deltaState);
  }
  notifySubscribers();
}

void Journal::addDelta(HashUpdateJournalDelta&& delta, const Hash& newHash) {
  {
    auto deltaState = deltaState_.wlock();

    // If the hashes were not set to anything, default to copying
    // the value from the prior journal entry
    if (delta.fromHash == kZeroHash) {
      delta.fromHash = deltaState->currentHash;
    }
    addDeltaWithoutNotifying(std::move(delta), *deltaState);
    deltaState->currentHash = newHash;
  }
  notifySubscribers();
}

std::optional<JournalDeltaInfo> Journal::getLatest() const {
  auto deltaState = deltaState_.rlock();
  if (deltaState->empty()) {
    return std::nullopt;
  } else {
    if (deltaState->isFileChangeInBack()) {
      const FileChangeJournalDelta& back = deltaState->fileChangeDeltas.back();
      return JournalDeltaInfo{deltaState->currentHash,
                              deltaState->currentHash,
                              back.sequenceID,
                              back.time};
    } else {
      const HashUpdateJournalDelta& back = deltaState->hashUpdateDeltas.back();
      return JournalDeltaInfo{
          back.fromHash, deltaState->currentHash, back.sequenceID, back.time};
    }
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

size_t Journal::estimateMemoryUsage() const {
  return estimateMemoryUsage(*deltaState_.rlock());
}

template <typename T>
size_t getPaddingAmount(const std::deque<T>& deltaDeque) {
  constexpr size_t numInDequeBuffer = 512 / sizeof(T);
  constexpr size_t maxBufSize = numInDequeBuffer * sizeof(T);
  size_t numBufs =
      (deltaDeque.size() + numInDequeBuffer - 1) / numInDequeBuffer;
  size_t padding = folly::goodMallocSize(maxBufSize) - maxBufSize;
  return padding * numBufs;
}

size_t Journal::estimateMemoryUsage(const DeltaState& deltaState) const {
  size_t memoryUsage = folly::goodMallocSize(sizeof(Journal));
  // Account for overhead of deques which have a maximum buffer size of 512.
  memoryUsage += getPaddingAmount(deltaState.fileChangeDeltas);
  memoryUsage += getPaddingAmount(deltaState.hashUpdateDeltas);

  if (deltaState.stats) {
    memoryUsage += deltaState.deltaMemoryUsage;
  }
  return memoryUsage;
}

void Journal::flush() {
  {
    auto deltaState = deltaState_.wlock();
    ++deltaState->nextSequence;
    auto lastHash = deltaState->currentHash;
    deltaState->fileChangeDeltas.clear();
    deltaState->hashUpdateDeltas.clear();
    deltaState->stats = std::nullopt;
    auto delta = HashUpdateJournalDelta();
    /* Tracking the hash correctly when the journal is flushed is important
     * since Watchman uses the hash to correctly determine what additional files
     * were changed when a checkout happens, journals have at least one entry
     * unless they are on the null commit with no modifications done. A flush
     * operation should leave us on the same checkout we were on before the
     * flush operation.
     */
    delta.fromHash = lastHash;
    addDeltaWithoutNotifying(std::move(delta), *deltaState);
  }
  notifySubscribers();
}

std::unique_ptr<JournalDeltaRange> Journal::accumulateRange() {
  return accumulateRange(1);
}

std::unique_ptr<JournalDeltaRange> Journal::accumulateRange(
    SequenceNumber from) {
  DCHECK(from > 0);
  std::unique_ptr<JournalDeltaRange> result = nullptr;

  size_t filesAccumulated = 0;
  auto deltaState = deltaState_.ulock();
  // If this is going to be truncated handle it before iterating.
  if (!deltaState->empty() && deltaState->getFrontSequenceID() > from) {
    result = std::make_unique<JournalDeltaRange>();
    result->isTruncated = true;
  } else {
    forEachDelta(
        *deltaState,
        from,
        std::nullopt,
        [&](const FileChangeJournalDelta& current) -> void {
          ++filesAccumulated;
          if (!result) {
            result = std::make_unique<JournalDeltaRange>();
            result->toSequence = current.sequenceID;
            result->toTime = current.time;
            result->toHash = deltaState->currentHash;
            result->fromHash = deltaState->currentHash;
          }
          // Capture the lower bound.
          result->fromSequence = current.sequenceID;
          result->fromTime = current.time;

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
        },
        [&](const HashUpdateJournalDelta& current) -> void {
          if (!result) {
            result = std::make_unique<JournalDeltaRange>();
            result->toSequence = current.sequenceID;
            result->toTime = current.time;
            result->toHash = deltaState->currentHash;
          }
          // Capture the lower bound.
          result->fromSequence = current.sequenceID;
          result->fromTime = current.time;
          result->fromHash = current.fromHash;

          // Merge the unclean status list
          result->uncleanPaths.insert(
              current.uncleanPaths.begin(), current.uncleanPaths.end());
        });
  }

  if (result) {
    if (edenStats_) {
      if (result->isTruncated) {
        edenStats_->getJournalStatsForCurrentThread().truncatedReads.addValue(
            1);
      }
      edenStats_->getJournalStatsForCurrentThread().filesAccumulated.addValue(
          filesAccumulated);
    }
    auto deltaStateWriter = deltaState.moveFromUpgradeToWrite();
    if (deltaStateWriter->stats) {
      deltaStateWriter->stats->maxFilesAccumulated = std::max(
          deltaStateWriter->stats->maxFilesAccumulated, filesAccumulated);
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
  Hash currentHash = deltaState->currentHash;
  forEachDelta(
      *deltaState,
      from,
      limit,
      [mountGeneration, &result, &currentHash](
          const FileChangeJournalDelta& current) -> void {
        DebugJournalDelta delta;
        JournalPosition fromPosition;
        fromPosition.set_mountGeneration(mountGeneration);
        fromPosition.set_sequenceNumber(current.sequenceID);
        fromPosition.set_snapshotHash(thriftHash(currentHash));
        delta.set_fromPosition(fromPosition);

        JournalPosition toPosition;
        toPosition.set_mountGeneration(mountGeneration);
        toPosition.set_sequenceNumber(current.sequenceID);
        toPosition.set_snapshotHash(thriftHash(currentHash));
        delta.set_toPosition(toPosition);

        for (const auto& entry : current.getChangedFilesInOverlay()) {
          auto& path = entry.first;
          auto& changeInfo = entry.second;

          DebugPathChangeInfo debugChangeInfo;
          debugChangeInfo.existedBefore = changeInfo.existedBefore;
          debugChangeInfo.existedAfter = changeInfo.existedAfter;
          delta.changedPaths.emplace(path.stringPiece().str(), debugChangeInfo);
        }

        result.push_back(delta);
      },
      [mountGeneration, &result, &currentHash](
          const HashUpdateJournalDelta& current) -> void {
        DebugJournalDelta delta;
        JournalPosition fromPosition;
        fromPosition.set_mountGeneration(mountGeneration);
        fromPosition.set_sequenceNumber(current.sequenceID);
        fromPosition.set_snapshotHash(thriftHash(current.fromHash));
        delta.set_fromPosition(fromPosition);

        JournalPosition toPosition;
        toPosition.set_mountGeneration(mountGeneration);
        toPosition.set_sequenceNumber(current.sequenceID);
        toPosition.set_snapshotHash(thriftHash(currentHash));
        delta.set_toPosition(toPosition);
        currentHash = current.fromHash;

        for (auto& path : current.uncleanPaths) {
          delta.uncleanPaths.emplace(path.stringPiece().str());
        }

        result.push_back(delta);
      });
  return result;
}

/**
 * FileChangeFunc: void(const FileChangeJournalDelta&)
 * HashUpdateFunc: void(const HashUpdateJournalDelta&)
 */
template <class FileChangeFunc, class HashUpdateFunc>
void Journal::forEachDelta(
    const DeltaState& deltaState,
    JournalDelta::SequenceNumber from,
    std::optional<size_t> lengthLimit,
    FileChangeFunc&& fileChangeDeltaCallback,
    HashUpdateFunc&& hashUpdateDeltaCallback) const {
  size_t iters = 0;
  auto fileChangeIt = deltaState.fileChangeDeltas.rbegin();
  auto hashUpdateIt = deltaState.hashUpdateDeltas.rbegin();
  auto fileChangeRend = deltaState.fileChangeDeltas.rend();
  auto hashUpdateRend = deltaState.hashUpdateDeltas.rend();
  while (fileChangeIt != fileChangeRend || hashUpdateIt != hashUpdateRend) {
    bool isFileChange = hashUpdateIt == hashUpdateRend ||
        (fileChangeIt != fileChangeRend &&
         fileChangeIt->sequenceID > hashUpdateIt->sequenceID);
    const Journal::SequenceNumber currentSequenceID =
        isFileChange ? fileChangeIt->sequenceID : hashUpdateIt->sequenceID;
    if (currentSequenceID < from) {
      break;
    }
    if (lengthLimit && iters >= lengthLimit.value()) {
      break;
    }
    if (isFileChange) {
      fileChangeDeltaCallback(*fileChangeIt);
      ++fileChangeIt;
    } else {
      hashUpdateDeltaCallback(*hashUpdateIt);
      ++hashUpdateIt;
    }

    ++iters;
  }
}
} // namespace eden
} // namespace facebook
