/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/journal/JournalDelta.h"
#include <folly/logging/xlog.h>
#include "eden/common/utils/Match.h"

namespace facebook::eden {

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    dtype_t type,
    FileChangeJournalDelta::Created)
    : path1{fileName.copy()},
      info1{PathChangeInfo{false, true}},
      isPath1Valid{true},
      type{type} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    dtype_t type,
    FileChangeJournalDelta::Removed)
    : path1{fileName.copy()},
      info1{PathChangeInfo{true, false}},
      isPath1Valid{true},
      type{type} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    dtype_t type,
    FileChangeJournalDelta::Changed)
    : path1{fileName.copy()},
      info1{PathChangeInfo{true, true}},
      isPath1Valid{true},
      type{type} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    dtype_t type,
    FileChangeJournalDelta::Renamed)
    : path1{oldName.copy()},
      path2{newName.copy()},
      info1{PathChangeInfo{true, false}},
      info2{PathChangeInfo{false, true}},
      isPath1Valid{true},
      isPath2Valid{true},
      type{type} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    dtype_t type,
    FileChangeJournalDelta::Replaced)
    : path1{oldName.copy()},
      path2{newName.copy()},
      info1{PathChangeInfo{true, false}},
      info2{PathChangeInfo{true, true}},
      isPath1Valid{true},
      isPath2Valid{true},
      type{type} {}

size_t FileChangeJournalDelta::estimateMemoryUsage() const {
  size_t mem = sizeof(FileChangeJournalDelta);

  /* NOTE: The following code assumes an unordered_set is separated into an
   * array of buckets, each one being a chain of nodes containing a next
   * pointer, a key-value pair, and a stored Id
   */
  if (isPath1Valid) {
    mem += facebook::eden::estimateIndirectMemoryUsage(path1);
  }
  if (isPath2Valid) {
    mem += facebook::eden::estimateIndirectMemoryUsage(path2);
  }

  return mem;
}

size_t RootUpdateJournalDelta::estimateMemoryUsage() const {
  size_t mem = sizeof(RootUpdateJournalDelta);

  /* NOTE: The following code assumes an unordered_set is separated into an
   * array of buckets, each one being a chain of nodes containing a next
   * pointer, a key-value pair, and a stored Id
   */

  // Calculate Memory For Nodes in Each Bucket (Pointer to element and next)
  size_t set_elem_size = folly::goodMallocSize(
      sizeof(void*) + sizeof(decltype(uncleanPaths)::value_type) +
      sizeof(size_t));
  for (unsigned long i = 0; i < uncleanPaths.bucket_count(); ++i) {
    mem += set_elem_size * uncleanPaths.bucket_size(i);
  }

  // Calculate Memory Usage of Bucket List
  mem += folly::goodMallocSize(sizeof(void*) * uncleanPaths.bucket_count());

  // Calculate Memory Usage used indirectly by elements
  for (auto& path : uncleanPaths) {
    mem += facebook::eden::estimateIndirectMemoryUsage(path);
  }

  return mem;
}

std::unordered_map<RelativePath, PathChangeInfo>
FileChangeJournalDelta::getChangedFilesInOverlay() const {
  std::unordered_map<RelativePath, PathChangeInfo> changedFilesInOverlay;
  if (isPath1Valid) {
    changedFilesInOverlay[path1] = info1;
  }
  if (isPath2Valid) {
    changedFilesInOverlay[path2] = info2;
  }
  return changedFilesInOverlay;
}

bool FileChangeJournalDelta::isModification() const {
  return isPath1Valid && !isPath2Valid && info1.existedBefore &&
      info1.existedAfter;
}

bool FileChangeJournalDelta::isSameAction(
    const FileChangeJournalDelta& other) const {
  return isPath1Valid == other.isPath1Valid && info1 == other.info1 &&
      path1 == other.path1 && isPath2Valid == other.isPath2Valid &&
      info2 == other.info2 && path2 == other.path2;
}

JournalDeltaPtr::JournalDeltaPtr(std::nullptr_t) {}

JournalDeltaPtr::JournalDeltaPtr(FileChangeJournalDelta* p) : data_{p} {
  XCHECK(p);
}

JournalDeltaPtr::JournalDeltaPtr(RootUpdateJournalDelta* p) : data_{p} {
  XCHECK(p);
}

size_t JournalDeltaPtr::estimateMemoryUsage() const {
  return match(
      data_,
      [](std::monostate) -> size_t { return 0; },
      [](auto* delta) { return delta->estimateMemoryUsage(); });
}

const JournalDelta* JournalDeltaPtr::operator->() const noexcept {
  return match(
      data_,
      [](std::monostate) -> JournalDelta* { return nullptr; },
      [](auto* delta) -> JournalDelta* { return delta; });
}

FileChangeJournalDelta* JournalDeltaPtr::getAsFileChangeJournalDelta() {
  return match(
      data_,
      [](FileChangeJournalDelta* p) { return p; },
      [](auto) -> FileChangeJournalDelta* { return nullptr; });
}

} // namespace facebook::eden
