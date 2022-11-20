/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/FutureSplitter.h>
#include <optional>
#include <string>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

class PrjfsDirEntry {
 public:
  PrjfsDirEntry() = delete;

  PrjfsDirEntry(
      PathComponentPiece name,
      bool isDir,
      ImmediateFuture<uint64_t> sizeFuture);

  /**
   * An entry whose size future has been resolved.
   */
  struct Ready {
    /** Name of the directory entry. */
    std::wstring name;
    /** Size of the file, 0 for a directory. */
    uint64_t size;
    /** Whether this entry is a directory. */
    bool isDir;
  };

  /**
   * Test whether this entry matches the given pattern.
   */
  bool matchPattern(const std::wstring& pattern) const;

  /**
   * Return a future that completes when the size of this entry becomes
   * available.
   */
  ImmediateFuture<Ready> getFuture();

  /**
   * Do a lexicographical comparison of the entry.
   *
   * Return true if this entry is lexicographically before the other.
   */
  bool operator<(const PrjfsDirEntry& other) const;

  /**
   * Return the name of this entry.
   */
  const std::wstring& getName() const {
    return name_;
  }

 private:
  std::wstring name_;
  folly::FutureSplitter<uint64_t> sizeFuture_;
  bool isDir_;
};

class Enumerator {
 public:
  Enumerator(const Enumerator&) = delete;
  Enumerator& operator=(const Enumerator&) = delete;

  explicit Enumerator(std::vector<PrjfsDirEntry> entryList);
  Enumerator(Enumerator&& other) = default;

  explicit Enumerator() = delete;

  std::vector<ImmediateFuture<PrjfsDirEntry::Ready>> getPendingDirEntries();

  void advanceEnumeration();

  void restartEnumeration() {
    iter_ = metadataList_.begin();
  }

  bool isSearchExpressionEmpty() const {
    return searchExpression_.empty();
  }

  void saveExpression(std::wstring searchExpression) noexcept {
    searchExpression_ = std::move(searchExpression);
  }

 private:
  std::wstring searchExpression_;
  std::vector<PrjfsDirEntry> metadataList_;

  /**
   * Iterator on the first directory entry that didn't get send to ProjectedFS.
   */
  std::vector<PrjfsDirEntry>::iterator iter_;
};
} // namespace facebook::eden
