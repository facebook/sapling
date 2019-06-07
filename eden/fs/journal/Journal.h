/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/Function.h>
#include <folly/Synchronized.h>
#include <algorithm>
#include <cstdint>
#include <memory>
#include <optional>
#include <unordered_map>
#include "eden/fs/journal/JournalDelta.h"

namespace facebook {
namespace eden {

/** Contains statistics about the current state of the journal */
struct JournalStats {
  size_t entryCount = 0;
  size_t memoryUsage = 0;
  std::chrono::steady_clock::time_point earliestTimestamp;
  std::chrono::steady_clock::time_point latestTimestamp;
};

/** The Journal exists to answer questions about how files are changing
 * over time.
 *
 * It contains metadata only; it is not a full snapshot of the state of
 * the filesystem at a particular point in time.
 * The intent is to be able query things like "which set of files changed
 * between time A and time B?".
 *
 * In the initial implementation we are recording file names from the overlay
 * but will expand this to record things like checking out different
 * revisions (the prior and new revision hash) from which we can derive
 * the larger list of files.
 *
 * The Journal class is thread-safe.  Subscribers are called on the thread
 * that called addDelta.
 */
class Journal {
 public:
  Journal() = default;

  /// It is almost always a mistake to copy a Journal.
  Journal(const Journal&) = delete;
  Journal& operator=(const Journal&) = delete;

  using SequenceNumber = JournalDelta::SequenceNumber;
  using SubscriberId = uint64_t;
  using SubscriberCallback = std::function<void()>;

  /** Add a delta to the journal
   * The delta will have a new sequence number and timestamp
   * applied. */
  void addDelta(std::unique_ptr<JournalDelta>&& delta);

  /** Get a shared, immutable reference to the tip of the journal.
   * May return nullptr if there have been no changes */
  JournalDeltaPtr getLatest() const;

  /** Replace the journal with a new delta.
   * The new delta will typically be the result of JournalDelta::merge().
   * No sanity checking is performed inside this function; the
   * supplied delta is moved in and replaces current tip. */
  void replaceJournal(std::unique_ptr<JournalDelta>&& delta);

  /** Register a subscriber.
   * A subscriber is just a callback that is called whenever the
   * journal has changed.
   * It is recommended that the subscriber callback do the minimal
   * amount of work needed to schedule the real work to happen in
   * some other context because journal updates are likely to happen
   * in awkward contexts or in the middle of some batch of mutations
   * where it is not appropriate to do any heavy lifting.
   * The return value of registerSubscriber is an identifier than
   * can be passed to cancelSubscriber to later remove the registration.
   */
  SubscriberId registerSubscriber(SubscriberCallback&& callback);
  void cancelSubscriber(SubscriberId id);

  void cancelAllSubscribers();
  bool isSubscriberValid(SubscriberId id) const;

  /** Returns an option that is nullopt if the Journal is empty or an option
   * that contains valid JournalStats if the Journal is non-empty*/
  std::optional<JournalStats> getStats();

 private:
  struct DeltaState {
    /** The sequence number that we'll use for the next entry
     * that we link into the chain */
    SequenceNumber nextSequence{1};
    /** The most recently recorded entry */
    JournalDeltaPtr latest;
  };
  folly::Synchronized<DeltaState> deltaState_;

  struct SubscriberState {
    SubscriberId nextSubscriberId{1};
    std::unordered_map<SubscriberId, SubscriberCallback> subscribers;
  };

  folly::Synchronized<SubscriberState> subscriberState_;
};
} // namespace eden
} // namespace facebook
