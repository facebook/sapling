/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "Journal.h"

#include <chrono>
#include <unordered_set>
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

class JournalDelta {
 public:
  JournalDelta() = default;
  JournalDelta(std::initializer_list<RelativePath> overlayFileNames);

  /** the prior delta and its chain */
  std::shared_ptr<const JournalDelta> previous;
  /** The current sequence range.
   * This is a range to accommodate merging a range into a single entry. */
  Journal::SequenceNumber fromSequence;
  Journal::SequenceNumber toSequence;
  /** The time at which the change was recorded.
   * This is a range to accommodate merging a range into a single entry. */
  std::chrono::steady_clock::time_point fromTime;
  std::chrono::steady_clock::time_point toTime;

  /** The set of files that changed in the overlay in this update */
  std::unordered_set<RelativePath> changedFilesInOverlay;

  /** Merge the deltas running back from this delta for all deltas
   * whose toSequence is >= limitSequence.
   * The default limit value is 0 which is never assigned by the Journal
   * and thus indicates that all deltas should be merged.
   * if pruneAfterLimit is true and we stop due to hitting limitSequence,
   * then the returned delta will have previous=nullptr rather than
   * maintaining the chain.
   * If the limitSequence means that no deltas will match, returns nullptr.
   * */
  std::unique_ptr<JournalDelta> merge(
      Journal::SequenceNumber limitSequence = 0,
      bool pruneAfterLimit = false) const;
};
}
}
