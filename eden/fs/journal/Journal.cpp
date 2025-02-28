/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/journal/Journal.h"
#include <folly/logging/xlog.h>
#include "eden/fs/journal/JournalDelta.h"

namespace facebook::eden {

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

bool Journal::DeltaState::isFileChangeInFront() const {
  bool isFileChangeEmpty = fileChangeDeltas.empty();
  bool isHashUpdateEmpty = hashUpdateDeltas.empty();
  if (!isFileChangeEmpty && !isHashUpdateEmpty) {
    return fileChangeDeltas.front().sequenceID <
        hashUpdateDeltas.front().sequenceID;
  }
  return !isFileChangeEmpty && isHashUpdateEmpty;
}

bool Journal::DeltaState::isFileChangeInBack() const {
  bool isFileChangeEmpty = fileChangeDeltas.empty();
  bool isHashUpdateEmpty = hashUpdateDeltas.empty();
  if (!isFileChangeEmpty && !isHashUpdateEmpty) {
    return fileChangeDeltas.back().sequenceID >
        hashUpdateDeltas.back().sequenceID;
  }
  return !isFileChangeEmpty && isHashUpdateEmpty;
}

void Journal::DeltaState::appendDelta(FileChangeJournalDelta&& delta) {
  fileChangeDeltas.emplace_back(std::move(delta));
}

void Journal::DeltaState::appendDelta(RootUpdateJournalDelta&& delta) {
  hashUpdateDeltas.emplace_back(std::move(delta));
}

Journal::Journal(EdenStatsPtr edenStats) : edenStats_{std::move(edenStats)} {
  // Add 0 so that this counter shows up in ODS
  edenStats_->increment(&JournalStats::truncatedReads, 0);
}

void Journal::recordCreated(RelativePathPiece fileName, dtype_t type) {
  addDelta(
      FileChangeJournalDelta(fileName, type, FileChangeJournalDelta::CREATED));
}

void Journal::recordRemoved(RelativePathPiece fileName, dtype_t type) {
  addDelta(
      FileChangeJournalDelta(fileName, type, FileChangeJournalDelta::REMOVED));
}

void Journal::recordChanged(RelativePathPiece fileName, dtype_t type) {
  addDelta(
      FileChangeJournalDelta(fileName, type, FileChangeJournalDelta::CHANGED));
}

void Journal::recordRenamed(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    dtype_t type) {
  addDelta(FileChangeJournalDelta(
      oldName, newName, type, FileChangeJournalDelta::RENAMED));
}

void Journal::recordReplaced(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    dtype_t type) {
  addDelta(FileChangeJournalDelta(
      oldName, newName, type, FileChangeJournalDelta::REPLACED));
}

void Journal::recordHashUpdate(RootId toHash) {
  addDelta(RootUpdateJournalDelta{}, std::move(toHash));
}

void Journal::recordHashUpdate(RootId fromHash, RootId toHash) {
  if (fromHash == toHash) {
    return;
  }
  RootUpdateJournalDelta delta;
  delta.fromHash = std::move(fromHash);
  addDelta(std::move(delta), toHash);
}

void Journal::recordUncleanPaths(
    RootId fromHash,
    RootId toHash,
    std::unordered_set<RelativePath> uncleanPaths) {
  if (fromHash == toHash && uncleanPaths.empty()) {
    return;
  }
  RootUpdateJournalDelta delta;
  delta.fromHash = std::move(fromHash);
  delta.uncleanPaths = std::move(uncleanPaths);
  addDelta(std::move(delta), std::move(toHash));
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
    RootUpdateJournalDelta& /* unused */,
    DeltaState& /* unused */) {
  return false;
}

template <typename T>
bool Journal::addDeltaBeforeNotifying(T&& delta, DeltaState& deltaState) {
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
      deltaState.stats = InternalJournalStats();
      deltaState.stats->entryCount = 1;
      deltaState.deltaMemoryUsage = delta.estimateMemoryUsage();
    }
    deltaState.stats->latestTimestamp = delta.time;
    deltaState.appendDelta(std::forward<T>(delta));
  }

  deltaState.stats->earliestTimestamp = deltaState.frontPtr()->time;

  bool shouldNotify = deltaState.lastModificationHasBeenObserved;
  deltaState.lastModificationHasBeenObserved = false;
  return shouldNotify;
}

void Journal::notifySubscribers() const {
  auto subscribers = subscriberState_.rlock()->subscribers;
  for (auto& sub : subscribers) {
    sub.second();
  }
}

void Journal::addDelta(FileChangeJournalDelta&& delta) {
  bool shouldNotify;
  {
    auto deltaState = deltaState_.lock();
    shouldNotify = addDeltaBeforeNotifying(std::move(delta), *deltaState);
  }
  if (shouldNotify) {
    notifySubscribers();
  }
}

void Journal::addDelta(RootUpdateJournalDelta&& delta, RootId newRootId) {
  bool shouldNotify;
  {
    auto deltaState = deltaState_.lock();

    // If the hashes were not set to anything, default to copying
    // the value from the prior journal entry
    if (delta.fromHash == RootId{}) {
      delta.fromHash = deltaState->currentHash;
    }
    shouldNotify = addDeltaBeforeNotifying(std::move(delta), *deltaState);
    deltaState->currentHash = std::move(newRootId);
  }
  if (shouldNotify) {
    notifySubscribers();
  }
}

std::optional<JournalDeltaInfo> Journal::getLatest() {
  auto deltaState = deltaState_.lock();
  deltaState->lastModificationHasBeenObserved = true;
  if (deltaState->empty()) {
    return std::nullopt;
  } else {
    if (deltaState->isFileChangeInBack()) {
      const FileChangeJournalDelta& back = deltaState->fileChangeDeltas.back();
      return JournalDeltaInfo{
          deltaState->currentHash,
          deltaState->currentHash,
          back.sequenceID,
          back.time};
    } else {
      const RootUpdateJournalDelta& back = deltaState->hashUpdateDeltas.back();
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

std::optional<InternalJournalStats> Journal::getStats() {
  return deltaState_.lock()->stats;
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
  auto deltaState = deltaState_.lock();
  deltaState->memoryLimit = limit;
}

size_t Journal::getMemoryLimit() const {
  auto deltaState = deltaState_.lock();
  return deltaState->memoryLimit;
}

size_t Journal::estimateMemoryUsage() const {
  return estimateMemoryUsage(*deltaState_.lock());
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
  // Account for overhead of dequeues which have a maximum buffer size of 512.
  memoryUsage += getPaddingAmount(deltaState.fileChangeDeltas);
  memoryUsage += getPaddingAmount(deltaState.hashUpdateDeltas);

  if (deltaState.stats) {
    memoryUsage += deltaState.deltaMemoryUsage;
  }
  return memoryUsage;
}

void Journal::flush() {
  bool shouldNotify;
  {
    auto deltaState = deltaState_.lock();
    ++deltaState->nextSequence;
    auto lastHash = deltaState->currentHash;
    deltaState->fileChangeDeltas.clear();
    deltaState->hashUpdateDeltas.clear();
    deltaState->stats = std::nullopt;
    auto delta = RootUpdateJournalDelta();
    /* Tracking the hash correctly when the journal is flushed is important
     * since Watchman uses the hash to correctly determine what additional files
     * were changed when a checkout happens, journals have at least one entry
     * unless they are on the null commit with no modifications done. A flush
     * operation should leave us on the same checkout we were on before the
     * flush operation.
     */
    delta.fromHash = lastHash;
    shouldNotify = addDeltaBeforeNotifying(std::move(delta), *deltaState);
  }
  if (shouldNotify) {
    notifySubscribers();
  }
}

std::unique_ptr<JournalDeltaRange> Journal::accumulateRange(
    SequenceNumber from) {
  XDCHECK(from > 0);
  std::unique_ptr<JournalDeltaRange> result = nullptr;
  folly::stop_watch<std::chrono::milliseconds> watch;

  size_t filesAccumulated = 0;
  auto deltaState = deltaState_.lock();
  // If this is going to be truncated, handle it before iterating.
  if (!deltaState->empty() && deltaState->getFrontSequenceID() > from) {
    result = std::make_unique<JournalDeltaRange>();
    result->isTruncated = true;
  } else {
    forEachDelta(
        *deltaState,
        from,
        std::nullopt,
        [&](const FileChangeJournalDelta& current) -> bool {
          ++filesAccumulated;
          if (!result) {
            result = std::make_unique<JournalDeltaRange>();
            result->toSequence = current.sequenceID;
            result->toTime = current.time;
            result->snapshotTransitions.push_back(deltaState->currentHash);
          }
          // Capture the lower bound.
          result->fromSequence = current.sequenceID;
          result->fromTime = current.time;

          for (auto& entry : current.getChangedFilesInOverlay()) {
            auto& name = entry.first;
            if (result->containsHgOnlyChanges && !name.empty() &&
                name.paths().begin().piece() != ".hg"_relpath) {
              result->containsHgOnlyChanges = false;
            }
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
          // Return value ignored here
          return true;
        },
        [&](const RootUpdateJournalDelta& current) -> bool {
          if (!result) {
            result = std::make_unique<JournalDeltaRange>();
            result->toSequence = current.sequenceID;
            result->toTime = current.time;
            result->snapshotTransitions.push_back(deltaState->currentHash);
          }
          // Capture the lower bound.
          result->fromSequence = current.sequenceID;
          result->fromTime = current.time;
          result->snapshotTransitions.push_back(current.fromHash);

          // Merge the unclean status list
          result->uncleanPaths.insert(
              current.uncleanPaths.begin(), current.uncleanPaths.end());
          // Return value ignored here
          return true;
        });
  }

  if (result) {
    if (edenStats_) {
      if (result->isTruncated) {
        edenStats_->increment(&JournalStats::truncatedReads);
      }
      edenStats_->increment(&JournalStats::filesAccumulated, filesAccumulated);
      edenStats_->addDuration(&JournalStats::accumulateRange, watch.elapsed());
    }
    if (deltaState->stats) {
      deltaState->stats->maxFilesAccumulated =
          std::max(deltaState->stats->maxFilesAccumulated, filesAccumulated);
    }

    std::reverse(
        result->snapshotTransitions.begin(), result->snapshotTransitions.end());
    result->containsRootUpdate = result->snapshotTransitions.size() > 1;
  }

  deltaState->lastModificationHasBeenObserved = true;
  return result;
}

bool Journal::forEachDelta(
    SequenceNumber from,
    std::optional<size_t> lengthLimit,
    FileChangeCallback&& fileChangeCallback,
    HashUpdateCallback&& hashUpdateCallback) {
  XDCHECK(from > 0);
  auto deltaState = deltaState_.lock();
  // If this is going to be truncated, handle it before iterating.
  if (!deltaState->empty() && deltaState->getFrontSequenceID() > from) {
    return true;
  } else {
    forEachDelta(
        *deltaState,
        from,
        lengthLimit,
        std::forward<FileChangeCallback>(fileChangeCallback),
        std::forward<HashUpdateCallback>(hashUpdateCallback));
  }
  deltaState->lastModificationHasBeenObserved = true;
  return false;
}

std::vector<DebugJournalDelta> Journal::getDebugRawJournalInfo(
    SequenceNumber from,
    std::optional<size_t> limit,
    long mountGeneration,
    RootIdCodec& rootIdCodec) const {
  auto result = std::vector<DebugJournalDelta>();
  auto deltaState = deltaState_.lock();
  RootId currentHash = deltaState->currentHash;
  forEachDelta(
      *deltaState,
      from,
      limit,
      [&](const FileChangeJournalDelta& current) -> bool {
        DebugJournalDelta delta;
        JournalPosition fromPosition;
        fromPosition.mountGeneration_ref() = mountGeneration;
        fromPosition.sequenceNumber_ref() = current.sequenceID;
        fromPosition.snapshotHash_ref() = rootIdCodec.renderRootId(currentHash);
        delta.fromPosition_ref() = fromPosition;

        JournalPosition toPosition;
        toPosition.mountGeneration_ref() = mountGeneration;
        toPosition.sequenceNumber_ref() = current.sequenceID;
        toPosition.snapshotHash_ref() = rootIdCodec.renderRootId(currentHash);
        delta.toPosition_ref() = toPosition;

        for (const auto& entry : current.getChangedFilesInOverlay()) {
          auto& path = entry.first;
          auto& changeInfo = entry.second;

          DebugPathChangeInfo debugChangeInfo;
          debugChangeInfo.existedBefore_ref() = changeInfo.existedBefore;
          debugChangeInfo.existedAfter_ref() = changeInfo.existedAfter;
          delta.changedPaths_ref()->emplace(path.asString(), debugChangeInfo);
        }

        result.push_back(delta);
        // Return value ignored here
        return true;
      },
      [&](const RootUpdateJournalDelta& current) -> bool {
        DebugJournalDelta delta;
        JournalPosition fromPosition;
        fromPosition.mountGeneration_ref() = mountGeneration;
        fromPosition.sequenceNumber_ref() = current.sequenceID;
        fromPosition.snapshotHash_ref() =
            rootIdCodec.renderRootId(current.fromHash);
        delta.fromPosition_ref() = fromPosition;

        JournalPosition toPosition;
        toPosition.mountGeneration_ref() = mountGeneration;
        toPosition.sequenceNumber_ref() = current.sequenceID;
        toPosition.snapshotHash_ref() = rootIdCodec.renderRootId(currentHash);
        delta.toPosition_ref() = toPosition;
        currentHash = current.fromHash;

        for (auto& path : current.uncleanPaths) {
          delta.uncleanPaths_ref()->emplace(path.asString());
        }

        result.push_back(delta);
        // Return value ignored here
        return true;
      });
  return result;
}

void Journal::forEachDelta(
    const DeltaState& deltaState,
    JournalDelta::SequenceNumber from,
    std::optional<size_t> lengthLimit,
    FileChangeCallback&& fileChangeDeltaCallback,
    HashUpdateCallback&& hashUpdateDeltaCallback) const {
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
      if (!fileChangeDeltaCallback(*fileChangeIt)) {
        break;
      };
      ++fileChangeIt;
    } else {
      if (!hashUpdateDeltaCallback(*hashUpdateIt)) {
        break;
      }
      ++hashUpdateIt;
    }

    ++iters;
  }
}
} // namespace facebook::eden
