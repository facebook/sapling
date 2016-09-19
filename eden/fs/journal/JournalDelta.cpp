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

std::unique_ptr<JournalDelta> JournalDelta::merge(
    Journal::SequenceNumber limitSequence,
    bool pruneAfterLimit) const {
  if (toSequence < limitSequence) {
    return nullptr;
  }

  const JournalDelta* current = this;
  auto result = std::make_unique<JournalDelta>();

  result->toSequence = current->toSequence;
  result->toTime = current->toTime;

  while (current) {
    if (current->toSequence < limitSequence) {
      break;
    }

    result->fromSequence = current->fromSequence;
    result->fromTime = current->fromTime;

    result->changedFilesInOverlay.insert(
        current->changedFilesInOverlay.begin(),
        current->changedFilesInOverlay.end());

    // Continue the chain, but not if the caller requested that
    // we prune it out.
    if (!pruneAfterLimit) {
      result->previous = current->previous;
    }

    current = current->previous.get();
  }

  return result;
}
}
}
