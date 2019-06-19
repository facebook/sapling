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

#include <chrono>
#include <unordered_set>
#include "eden/fs/journal/JournalDeltaPtr.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

struct PathChangeInfo {
  PathChangeInfo() : existedBefore{false}, existedAfter{false} {}

  PathChangeInfo(bool before, bool after)
      : existedBefore{before}, existedAfter{after} {}

  bool isNew() const {
    return !existedBefore && existedAfter;
  }

  /// Whether this path existed at the start of this delta.
  bool existedBefore : 1;

  /**
   * Whether this path existed at the end of this delta.
   * If existedAfter && !existedBefore, then the file can be considered new in
   * this delta.
   */
  bool existedAfter : 1;

  // TODO: It may make sense to maintain an existenceChanged bit to distinguish
  // between a file being changed and it being removed and added in the same
  // delta.
};

class JournalDelta {
 public:
  using SequenceNumber = uint64_t;

  enum Created { CREATED };
  enum Removed { REMOVED };
  enum Changed { CHANGED };
  enum Renamed { RENAMED };
  enum Replaced { REPLACED };
  JournalDelta() = default;
  JournalDelta(JournalDelta&&) = delete;
  JournalDelta& operator=(JournalDelta&&) = delete;
  JournalDelta(const JournalDelta&) = delete;
  JournalDelta& operator=(const JournalDelta&) = delete;
  JournalDelta(RelativePathPiece fileName, Created);
  JournalDelta(RelativePathPiece fileName, Removed);
  JournalDelta(RelativePathPiece fileName, Changed);

  /**
   * "Renamed" means that that newName was created as a result of the mv(1).
   */
  JournalDelta(RelativePathPiece oldName, RelativePathPiece newName, Renamed);

  /**
   * "Replaced" means that that newName was overwritten by oldName as a result
   * of the mv(1).
   */
  JournalDelta(RelativePathPiece oldName, RelativePathPiece newName, Replaced);

  ~JournalDelta();

  /** the prior delta and its chain */
  JournalDeltaPtr previous;
  /** The current sequence range.
   * This is a range to accommodate merging a range into a single entry. */
  SequenceNumber fromSequence;
  SequenceNumber toSequence;
  /** The time at which the change was recorded.
   * This is a range to accommodate merging a range into a single entry. */
  std::chrono::steady_clock::time_point fromTime;
  std::chrono::steady_clock::time_point toTime;

  /** The snapshot hash that we started and ended up on.
   * This will often be the same unless we perform a checkout or make
   * a new snapshot from the snapshotable files in the overlay. */
  Hash fromHash;
  Hash toHash;

  /**
   * The set of files that changed in the overlay in this update, including
   * some information about the changes.
   */
  std::unordered_map<RelativePath, PathChangeInfo> changedFilesInOverlay;
  /** The set of files that had differing status across a checkout or
   * some other operation that changes the snapshot hash */
  std::unordered_set<RelativePath> uncleanPaths;

  /** Get memory used (in bytes) by this Delta */
  size_t estimateMemoryUsage() const;

 private:
  void incRef() const noexcept;
  void decRef() const noexcept;
  bool isUnique() const noexcept;

  mutable std::atomic<size_t> refCount_{0};

  // For reference counting.
  friend class JournalDeltaPtr;
};

struct JournalDeltaRange {
  /** The current sequence range.
   * This is a range to accommodate merging a range into a single entry. */
  JournalDelta::SequenceNumber fromSequence;
  JournalDelta::SequenceNumber toSequence;
  /** The time at which the change was recorded.
   * This is a range to accommodate merging a range into a single entry. */
  std::chrono::steady_clock::time_point fromTime;
  std::chrono::steady_clock::time_point toTime;

  /** The snapshot hash that we started and ended up on.
   * This will often be the same unless we perform a checkout or make
   * a new snapshot from the snapshotable files in the overlay. */
  Hash fromHash;
  Hash toHash;

  /**
   * The set of files that changed in the overlay in this update, including
   * some information about the changes.
   */
  std::unordered_map<RelativePath, PathChangeInfo> changedFilesInOverlay;
  /** The set of files that had differing status across a checkout or
   * some other operation that changes the snapshot hash */
  std::unordered_set<RelativePath> uncleanPaths;

  bool isTruncated = false;
  JournalDeltaRange() = default;
  JournalDeltaRange(JournalDeltaRange&&) = default;
  JournalDeltaRange& operator=(JournalDeltaRange&&) = default;
};

} // namespace eden
} // namespace facebook
