/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "JournalDelta.h"
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    FileChangeJournalDelta::Created)
    : path1{fileName.copy()},
      info1{PathChangeInfo{false, true}},
      isPath1Valid{true} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    FileChangeJournalDelta::Removed)
    : path1{fileName.copy()},
      info1{PathChangeInfo{true, false}},
      isPath1Valid{true} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece fileName,
    FileChangeJournalDelta::Changed)
    : path1{fileName.copy()},
      info1{PathChangeInfo{true, true}},
      isPath1Valid{true} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    FileChangeJournalDelta::Renamed)
    : path1{oldName.copy()},
      path2{newName.copy()},
      info1{PathChangeInfo{true, false}},
      info2{PathChangeInfo{false, true}},
      isPath1Valid{true},
      isPath2Valid{true} {}

FileChangeJournalDelta::FileChangeJournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    FileChangeJournalDelta::Replaced)
    : path1{oldName.copy()},
      path2{newName.copy()},
      info1{PathChangeInfo{true, false}},
      info2{PathChangeInfo{true, true}},
      isPath1Valid{true},
      isPath2Valid{true} {}

size_t FileChangeJournalDelta::estimateMemoryUsage() const {
  size_t mem = sizeof(FileChangeJournalDelta);

  /* NOTE: The following code assumes an unordered_set is separated into an
   * array of buckets, each one being a chain of nodes containing a next
   * pointer, a key-value pair, and a stored hash
   */
  if (isPath1Valid) {
    mem += facebook::eden::estimateIndirectMemoryUsage(path1);
  }
  if (isPath2Valid) {
    mem += facebook::eden::estimateIndirectMemoryUsage(path2);
  }

  return mem;
}

size_t HashUpdateJournalDelta::estimateMemoryUsage() const {
  size_t mem = sizeof(HashUpdateJournalDelta);

  /* NOTE: The following code assumes an unordered_set is separated into an
   * array of buckets, each one being a chain of nodes containing a next
   * pointer, a key-value pair, and a stored hash
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
  CHECK(p);
}

JournalDeltaPtr::JournalDeltaPtr(HashUpdateJournalDelta* p) : data_{p} {
  CHECK(p);
}

size_t JournalDeltaPtr::estimateMemoryUsage() const {
  return std::visit(
      [](auto delta) -> size_t {
        if constexpr (std::is_same_v<decltype(delta), std::monostate>) {
          return 0;
        } else {
          return delta->estimateMemoryUsage();
        }
      },
      data_);
}

const JournalDelta* JournalDeltaPtr::operator->() const noexcept {
  return std::visit(
      [](auto delta) -> JournalDelta* {
        if constexpr (std::is_same_v<decltype(delta), std::monostate>) {
          return nullptr;
        } else {
          return delta;
        }
      },
      data_);
}

FileChangeJournalDelta* JournalDeltaPtr::getAsFileChangeJournalDelta() {
  return std::visit(
      [](auto delta) -> FileChangeJournalDelta* {
        if constexpr (std::
                          is_same_v<decltype(delta), FileChangeJournalDelta*>) {
          return delta;
        } else {
          return nullptr;
        }
      },
      data_);
}

} // namespace eden

} // namespace facebook
