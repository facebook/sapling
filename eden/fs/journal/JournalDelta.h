/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <type_traits>
#include <unordered_set>
#include <variant>
#include "eden/common/utils/DirType.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/model/RootId.h"

namespace facebook::eden {

struct PathChangeInfo {
  PathChangeInfo() : existedBefore{false}, existedAfter{false} {}

  PathChangeInfo(bool before, bool after)
      : existedBefore{before}, existedAfter{after} {}

  bool isNew() const {
    return !existedBefore && existedAfter;
  }

  bool operator==(const PathChangeInfo& other) const {
    return existedBefore == other.existedBefore &&
        existedAfter == other.existedAfter;
  }

  bool operator!=(const PathChangeInfo& other) const {
    return !(*this == other);
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

  /** The ID of this Delta in the Journal */
  JournalDelta::SequenceNumber sequenceID;
  /** The time at which the change was recorded. */
  std::chrono::steady_clock::time_point time;
};

/** A delta that stores information about changed files */
class FileChangeJournalDelta : public JournalDelta {
 public:
  enum Created { CREATED };
  enum Removed { REMOVED };
  enum Changed { CHANGED };
  enum Renamed { RENAMED };
  enum Replaced { REPLACED };

  FileChangeJournalDelta() = default;
  FileChangeJournalDelta(FileChangeJournalDelta&&) = default;
  FileChangeJournalDelta& operator=(FileChangeJournalDelta&&) = default;
  FileChangeJournalDelta(const FileChangeJournalDelta&) = delete;
  FileChangeJournalDelta& operator=(const FileChangeJournalDelta&) = delete;
  FileChangeJournalDelta(RelativePathPiece fileName, dtype_t type, Created);
  FileChangeJournalDelta(RelativePathPiece fileName, dtype_t type, Removed);
  FileChangeJournalDelta(RelativePathPiece fileName, dtype_t type, Changed);

  /**
   * "Renamed" means that that newName was created as a result of the mv(1).
   */
  FileChangeJournalDelta(
      RelativePathPiece oldName,
      RelativePathPiece newName,
      dtype_t type,
      Renamed);

  /**
   * "Replaced" means that that newName was overwritten by oldName as a result
   * of the mv(1).
   */
  FileChangeJournalDelta(
      RelativePathPiece oldName,
      RelativePathPiece newName,
      dtype_t type,
      Replaced);

  /** Which of these paths actually contain information */
  RelativePath path1;
  RelativePath path2;
  PathChangeInfo info1;
  PathChangeInfo info2;
  bool isPath1Valid = false;
  bool isPath2Valid = false;

  /**
   * What type of paths do these represent
   * Single file operations (Created, Removed, Changed) are obvious.
   * Rename does not change the type of the file, but the name - so the type
   * does not change.
   * Replace overwrites one file with another, so the type is that of the
   * source.
   */
  dtype_t type;

  std::unordered_map<RelativePath, PathChangeInfo> getChangedFilesInOverlay()
      const;

  /** Checks whether this delta is a modification */
  bool isModification() const;

  /** Checks whether this delta and other are the same disregarding time and
   * sequenceID [whether they do the same action] */
  bool isSameAction(const FileChangeJournalDelta& other) const;

  /** Get memory used (in bytes) by this Delta */
  size_t estimateMemoryUsage() const;
};

/** A delta that stores information about changing commits */
class RootUpdateJournalDelta : public JournalDelta {
 public:
  RootUpdateJournalDelta() = default;
  RootUpdateJournalDelta(RootUpdateJournalDelta&&) = default;
  RootUpdateJournalDelta& operator=(RootUpdateJournalDelta&&) = default;
  RootUpdateJournalDelta(const RootUpdateJournalDelta&) = delete;
  RootUpdateJournalDelta& operator=(const RootUpdateJournalDelta&) = delete;

  /**
   * The snapshot id that we started and ended up on.
   * This will often be the same unless we perform a checkout or make
   * a new snapshot from the snapshotable files in the overlay.
   */
  RootId fromRoot;

  /** The set of files that had differing status across a checkout or
   * some other operation that changes the snapshot id */
  std::unordered_set<RelativePath> uncleanPaths;

  /** Get memory used (in bytes) by this Delta */
  size_t estimateMemoryUsage() const;
};

class JournalDeltaPtr {
 public:
  /* implicit */ JournalDeltaPtr(std::nullptr_t);

  /* implicit */ JournalDeltaPtr(FileChangeJournalDelta* p);

  /* implicit */ JournalDeltaPtr(RootUpdateJournalDelta* p);

  size_t estimateMemoryUsage() const;

  explicit operator bool() const noexcept {
    return !std::holds_alternative<std::monostate>(data_);
  }

  /** If this JournalDeltaPtr points to a FileChangeJournalDelta then returns
   * the raw pointer, if it does not point to a FileChangeJournalDelta then
   * return nullptr. */
  FileChangeJournalDelta* getAsFileChangeJournalDelta();

  const JournalDelta* operator->() const noexcept;

 private:
  std::variant<std::monostate, FileChangeJournalDelta*, RootUpdateJournalDelta*>
      data_;
};

struct JournalDeltaRange {
  /**
   * The current sequence range.
   * This is a range to accommodate merging a range into a single entry.
   */
  JournalDelta::SequenceNumber fromSequence;
  JournalDelta::SequenceNumber toSequence;

  /**
   * The time at which the change was recorded.
   * This is a range to accommodate merging a range into a single entry.
   */
  std::chrono::steady_clock::time_point fromTime;
  std::chrono::steady_clock::time_point toTime;

  /**
   * If the current commit did not change during this window, the list contains
   * one entry: the current snapshot id. If the range contains an update
   * operation, the list contains two entries: the previous id and the new
   * id. If two commits have occurred in this range, there are three entries,
   * and so on.
   *
   * This entries in this list are not unique. [A, B, C] is different than [A,
   * C, B], and [A, B, A] is common too.
   */
  std::vector<RootId> snapshotTransitions;

  /**
   * The set of files that changed in the overlay in this update, including
   * some information about the changes.
   */
  std::unordered_map<RelativePath, PathChangeInfo> changedFilesInOverlay;
  /** The set of files that had differing status across a checkout or
   * some other operation that changes the snapshot id */
  std::unordered_set<RelativePath> uncleanPaths;

  bool isTruncated = false;
  bool containsHgOnlyChanges = true;
  bool containsRootUpdate = false;
  JournalDeltaRange() {
    // 1 and 2 entries are the most common by far.
    snapshotTransitions.reserve(2);
  }
  JournalDeltaRange(JournalDeltaRange&&) = default;
  JournalDeltaRange& operator=(JournalDeltaRange&&) = default;
};

} // namespace facebook::eden
