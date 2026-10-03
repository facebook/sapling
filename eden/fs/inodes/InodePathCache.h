/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <folly/container/EvictingCacheMap.h>

#include <atomic>
#include <cstdint>
#include <mutex>
#include <optional>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeNumber.h"

namespace facebook::eden {

/**
 * Remembers the paths of recently used directories so that an inode's path
 * can be built from its parent's path instead of walked up to the root.
 *
 * Every entry carries the generation that was current when it was computed,
 * and any change to a directory's location bumps the generation. An entry from
 * an older generation is never returned, so a prefix computed before a rename
 * or rmdir is never joined to a name read after it.
 */
class InodePathCache {
 public:
  /**
   * A capacity of 0 disables the cache: enabled() is false and find() and
   * insert() must not be called.
   */
  explicit InodePathCache(size_t capacity);

  bool enabled() const {
    return capacity_ > 0;
  }

  uint64_t generation() const {
    return generation_.load(std::memory_order_acquire);
  }

  /**
   * True while a directory's location is being changed. Entries are neither
   * recorded nor served then: one recorded before the change wrote the new
   * location could otherwise be served after it, until the closing bump.
   */
  bool changing() const {
    return changesInFlight_.load(std::memory_order_acquire) != 0;
  }

  /**
   * Bracket a directory's location change. Each call moves to a new
   * generation, retiring every entry recorded so far, so an entry computed
   * while the change was in flight is retired along with the ones computed
   * before it.
   */
  void beginChange() {
    changesInFlight_.fetch_add(1, std::memory_order_acq_rel);
    generation_.fetch_add(1, std::memory_order_acq_rel);
  }
  void endChange() {
    generation_.fetch_add(1, std::memory_order_acq_rel);
    changesInFlight_.fetch_sub(1, std::memory_order_acq_rel);
  }

  /**
   * Return the path of directory `ino` if it was recorded under `generation`.
   */
  std::optional<RelativePath> find(InodeNumber ino, uint64_t generation);

  void insert(InodeNumber ino, RelativePathPiece path, uint64_t generation);

 private:
  struct Entry {
    uint64_t generation;
    RelativePath path;
  };

  const size_t capacity_;
  std::atomic<uint64_t> generation_{0};
  std::atomic<uint32_t> changesInFlight_{0};
  folly::Synchronized<folly::EvictingCacheMap<InodeNumber, Entry>, std::mutex>
      entries_;
};

} // namespace facebook::eden
