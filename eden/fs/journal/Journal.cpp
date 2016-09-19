/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "JournalDelta.h"

namespace facebook {
namespace eden {

void Journal::addDelta(std::unique_ptr<JournalDelta>&& delta) {
  delta->toSequence = nextSequence_++;
  delta->fromSequence = delta->toSequence;

  delta->toTime = std::chrono::steady_clock::now();
  delta->fromTime = delta->toTime;

  delta->previous = latest_;

  // If the hashes were not set to anything, default to copying
  // the value from the prior journal entry
  if (delta->previous && delta->fromHash == kZeroHash &&
      delta->toHash == kZeroHash) {
    delta->fromHash = delta->previous->toHash;
    delta->toHash = delta->fromHash;
  }

  latest_ = std::shared_ptr<const JournalDelta>(std::move(delta));
}

std::shared_ptr<const JournalDelta> Journal::getLatest() const {
  return latest_;
}

void Journal::replaceJournal(std::unique_ptr<JournalDelta>&& delta) {
  latest_ = std::shared_ptr<const JournalDelta>(std::move(delta));
}
}
}
