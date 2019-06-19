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

JournalDelta::JournalDelta(RelativePathPiece fileName, JournalDelta::Created)
    : changedFilesInOverlay{{fileName.copy(), PathChangeInfo{false, true}}} {}

JournalDelta::JournalDelta(RelativePathPiece fileName, JournalDelta::Removed)
    : changedFilesInOverlay{{fileName.copy(), PathChangeInfo{true, false}}} {}

JournalDelta::JournalDelta(RelativePathPiece fileName, JournalDelta::Changed)
    : changedFilesInOverlay{{fileName.copy(), PathChangeInfo{true, true}}} {}

JournalDelta::JournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    JournalDelta::Renamed)
    : changedFilesInOverlay{{oldName.copy(), PathChangeInfo{true, false}},
                            {newName.copy(), PathChangeInfo{false, true}}} {}

JournalDelta::JournalDelta(
    RelativePathPiece oldName,
    RelativePathPiece newName,
    JournalDelta::Replaced)
    : changedFilesInOverlay{{oldName.copy(), PathChangeInfo{true, false}},
                            {newName.copy(), PathChangeInfo{true, true}}} {}

JournalDelta::~JournalDelta() {
  // O(1) stack space destruction of the delta chain.
  JournalDeltaPtr p{std::move(previous)};
  while (p && p.unique()) {
    // We know we have the only reference to p, so cast away constness because
    // we need to unset p->previous.
    JournalDelta* q = const_cast<JournalDelta*>(p.get());
    p = std::move(q->previous);
  }
}

size_t JournalDelta::estimateMemoryUsage() const {
  size_t mem = folly::goodMallocSize(sizeof(JournalDelta));
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

void JournalDelta::incRef() const noexcept {
  refCount_.fetch_add(1, std::memory_order_relaxed);
}

void JournalDelta::decRef() const noexcept {
  if (1 == refCount_.fetch_sub(1, std::memory_order_acq_rel)) {
    delete this;
  }
}

bool JournalDelta::isUnique() const noexcept {
  return 1 == refCount_.load(std::memory_order_acquire);
}

} // namespace eden
} // namespace facebook
