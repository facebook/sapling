/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "JournalDelta.h"
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

JournalDelta::JournalDelta(std::initializer_list<RelativePath> overlayFileNames)
    : changedFilesInOverlay(overlayFileNames) {}

JournalDelta::JournalDelta(RelativePathPiece fileName, JournalDelta::Created)
    : createdFilesInOverlay({fileName.copy()}) {}

JournalDelta::JournalDelta(RelativePathPiece fileName, JournalDelta::Removed)
    : removedFilesInOverlay({fileName.copy()}) {}

JournalDelta::JournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    JournalDelta::Renamed)
    : createdFilesInOverlay({newName.copy()}),
      removedFilesInOverlay({oldName.copy()}) {}

std::unique_ptr<JournalDelta> JournalDelta::merge(
    Journal::SequenceNumber limitSequence,
    bool pruneAfterLimit) const {
  if (toSequence < limitSequence) {
    return nullptr;
  }

  const JournalDelta* current = this;

  // To help satisfy ourselves that we don't ever emit a merged
  // JournalDelta that has a given file name in more than one of
  // the createdFilesInOverlay, removedFilesInOverlay or changedFilesInOverlay
  // sets, we first build up a map of name -> state and then apply
  // the state transitions to it.  Keep in mind that we are processing
  // from the most recent event first, so the transitions are backwards.
  // The comments in the loop below show the forwards ordering.
  enum Disposition {
    Created,
    Changed,
    Removed,
  };
  std::unordered_map<RelativePath, Disposition> overlayState;
  auto result = std::make_unique<JournalDelta>();

  result->toSequence = current->toSequence;
  result->toTime = current->toTime;
  result->fromHash = fromHash;
  result->toHash = toHash;

  while (current) {
    if (current->toSequence < limitSequence) {
      break;
    }

    // Capture the lower bound.
    result->fromSequence = current->fromSequence;
    result->fromTime = current->fromTime;
    result->fromHash = current->fromHash;

    // Merge the unclean status list
    result->uncleanPaths.insert(
        current->uncleanPaths.begin(), current->uncleanPaths.end());

    // process created files.
    for (auto& created : current->createdFilesInOverlay) {
      auto it = overlayState.find(created);
      if (it == overlayState.end()) {
        overlayState[created] = Created;
      } else {
        switch (it->second) {
          case Changed:
            // Created, Changed -> Created
            overlayState[created] = Created;
            break;
          case Removed:
            // Created, Removed -> cancel out (don't report)
            overlayState.erase(it);
            break;
          case Created:
            // Created, Created -> Created
            XLOG(ERR) << "Journal for " << created
                      << " holds invalid Created, Created sequence";
            break;
        }
      }
    }

    // process removed files.
    for (auto& removed : current->removedFilesInOverlay) {
      const auto& it = overlayState.find(removed);
      if (it == overlayState.end()) {
        overlayState[removed] = Removed;
      } else {
        switch (it->second) {
          case Created:
            // Removed, Created -> cancel out to Changed
            overlayState[removed] = Changed;
            break;
          case Changed:
            // Removed, Changed -> invalid
            XLOG(ERR) << "Journal for " << removed
                      << " holds invalid Removed, Changed sequence";
            break;
          case Removed:
            // Removed, Removed -> Removed
            XLOG(ERR) << "Journal for " << removed
                      << " holds invalid Removed, Removed sequence";
            break;
        }
      }
    }

    // process changed files.
    for (auto& changed : current->changedFilesInOverlay) {
      const auto& it = overlayState.find(changed);
      if (it == overlayState.end()) {
        overlayState[changed] = Changed;
      } else {
        switch (it->second) {
          case Created:
            // Changed, Created -> invalid
            XLOG(ERR) << "Journal for " << changed
                      << " holds invalid Changed, Created sequence";
            break;
          case Changed:
            // Changed, Changed -> Changed
            break;
          case Removed:
            // Changed, Removed -> Removed
            break;
        }
      }
    }

    // Continue the chain, but not if the caller requested that
    // we prune it out.
    if (!pruneAfterLimit) {
      result->previous = current->previous;
    }

    current = current->previous.get();
  }

  // Now translate the keys of the state into entries in one
  // of the three sets for the result.
  for (const auto& it : overlayState) {
    const auto& fileName = it.first;
    switch (it.second) {
      case Created:
        result->createdFilesInOverlay.insert(fileName);
        break;
      case Changed:
        result->changedFilesInOverlay.insert(fileName);
        break;
      case Removed:
        result->removedFilesInOverlay.insert(fileName);
        break;
    }
  }

  return result;
}
} // namespace eden
} // namespace facebook
