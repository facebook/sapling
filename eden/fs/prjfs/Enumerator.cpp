/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/prjfs/Enumerator.h"

#include <ProjectedFSLib.h> // @manual
#include <folly/executors/GlobalExecutor.h>
#include <folly/portability/Windows.h>

namespace facebook::eden {

PrjfsDirEntry::PrjfsDirEntry(
    PathComponentPiece name,
    bool isDir,
    ImmediateFuture<uint64_t> sizeFuture)
    : name_{name.wide()},
      // In the case where the future isn't ready yet, we want to start
      // driving it immediately, thus convert it to a Future.
      sizeFuture_{
          std::move(sizeFuture).semi().via(folly::getGlobalCPUExecutor())},
      isDir_{isDir} {}

bool PrjfsDirEntry::matchPattern(const std::wstring& pattern) const {
  return PrjFileNameMatch(name_.c_str(), pattern.c_str());
}

ImmediateFuture<PrjfsDirEntry::Ready> PrjfsDirEntry::getFuture() {
  return ImmediateFuture{sizeFuture_.getSemiFuture()}.thenValue(
      [name = name_, isDir = isDir_](uint64_t size) {
        return Ready{std::move(name), size, isDir};
      });
}

bool PrjfsDirEntry::operator<(const PrjfsDirEntry& other) const {
  return PrjFileNameCompare(name_.c_str(), other.name_.c_str()) < 0;
}

Enumerator::Enumerator(std::vector<PrjfsDirEntry> entryList)
    : metadataList_(std::move(entryList)), iter_{metadataList_.begin()} {
  std::sort(
      metadataList_.begin(),
      metadataList_.end(),
      [](const PrjfsDirEntry& first, const PrjfsDirEntry& second) -> bool {
        return first < second;
      });
}

void Enumerator::advanceEnumeration() {
  XDCHECK_NE(iter_, metadataList_.end());

  while (iter_ != metadataList_.end() &&
         !iter_->matchPattern(searchExpression_)) {
    ++iter_;
  }

  if (iter_ == metadataList_.end()) {
    return;
  }

  ++iter_;
}

std::vector<ImmediateFuture<PrjfsDirEntry::Ready>>
Enumerator::getPendingDirEntries() {
  std::vector<ImmediateFuture<PrjfsDirEntry::Ready>> ret;
  for (auto it = iter_; it != metadataList_.end(); it++) {
    if (it->matchPattern(searchExpression_)) {
      ret.push_back(it->getFuture());
    }
  }
  return ret;
}

} // namespace facebook::eden

#endif
