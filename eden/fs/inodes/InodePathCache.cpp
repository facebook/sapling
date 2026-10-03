/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodePathCache.h"

#include <algorithm>

namespace facebook::eden {

InodePathCache::InodePathCache(size_t capacity)
    : capacity_{capacity},
      entries_{std::in_place, std::max<size_t>(capacity, 1)} {}

std::optional<RelativePath> InodePathCache::find(
    InodeNumber ino,
    uint64_t generation) {
  auto entries = entries_.lock();
  auto iter = entries->find(ino);
  if (iter == entries->end() || iter->second.generation != generation) {
    return std::nullopt;
  }
  return iter->second.path;
}

void InodePathCache::insert(
    InodeNumber ino,
    RelativePathPiece path,
    uint64_t generation) {
  entries_.lock()->set(ino, Entry{generation, path.copy()});
}

} // namespace facebook::eden
