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

Enumeration::Enumeration(std::vector<PrjfsDirEntry::Ready> dirEntries)
    : dirEntries_(std::move(dirEntries)), iter_{dirEntries_.begin()} {}

Enumerator::Enumerator(std::vector<PrjfsDirEntry> entryList)
    : metadataList_(std::move(entryList)) {
  std::sort(
      metadataList_.begin(),
      metadataList_.end(),
      [](const PrjfsDirEntry& first, const PrjfsDirEntry& second) -> bool {
        return first < second;
      });
}

ImmediateFuture<std::shared_ptr<Enumeration>> Enumerator::prepareEnumeration() {
  if (enumeration_) {
    return ImmediateFuture<std::shared_ptr<Enumeration>>(enumeration_);
  }

  std::vector<ImmediateFuture<PrjfsDirEntry::Ready>> pendingDirEntries;
  pendingDirEntries.reserve(metadataList_.size());
  for (auto& entry : metadataList_) {
    if (entry.matchPattern(searchExpression_)) {
      pendingDirEntries.push_back(entry.getFuture());
    }
  }
  return collectAllSafe(std::move(pendingDirEntries))
      .thenValue([this](std::vector<PrjfsDirEntry::Ready> dirEntries) {
        enumeration_ = std::make_shared<Enumeration>(dirEntries);
        return enumeration_;
      });
}

} // namespace facebook::eden

#endif
