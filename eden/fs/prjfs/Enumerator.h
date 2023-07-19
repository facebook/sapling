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
      std::optional<ImmediateFuture<std::string>> symlinkTarget,
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
    /** Optional symlink target for symlinks */
    std::optional<std::string> symlinkTarget;
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
  std::optional<folly::FutureSplitter<std::string>> symlinkTarget_;
};

/**
 * A single enumeration over directory entries.
 */
class Enumeration {
 public:
  Enumeration(const Enumeration&) = delete;
  Enumeration& operator=(const Enumeration&) = delete;

  explicit Enumeration(std::vector<PrjfsDirEntry::Ready> dirEntries);
  Enumeration(Enumeration&& other) = default;

  /**
   * Gets the current directory entry, or nullopt if we've reached the end of
   * enumeration.
   */
  inline std::optional<PrjfsDirEntry::Ready> getCurrent() {
    if (iter_ == dirEntries_.end()) {
      return std::nullopt;
    }
    return *iter_;
  }

  /**
   * Advances the enumeration and gets the next directory entry, or nullopt if
   * we've then reached the end of enumeration.
   */
  inline std::optional<PrjfsDirEntry::Ready> getNext() {
    if (iter_ == dirEntries_.end()) {
      XLOG(FATAL) << "Attempted to iterate past end of ProjFS Enumerator";
    }
    ++iter_;
    return getCurrent();
  }

 private:
  std::vector<PrjfsDirEntry::Ready> dirEntries_;
  std::vector<PrjfsDirEntry::Ready>::iterator iter_;
};

class Enumerator {
 public:
  Enumerator(const Enumerator&) = delete;
  Enumerator& operator=(const Enumerator&) = delete;

  explicit Enumerator(std::vector<PrjfsDirEntry> entryList);
  Enumerator(Enumerator&& other) = default;

  explicit Enumerator() = delete;

  std::vector<ImmediateFuture<PrjfsDirEntry::Ready>> getPendingDirEntries();

  /**
   * Prepares an Enumeration using the current search expression.
   *
   * Not reentrant: We rely on PrjFS's behavior of only requesting batches of
   * directory entries sequentially.
   */
  ImmediateFuture<std::shared_ptr<Enumeration>> prepareEnumeration();

  /**
   * Restarts enumeration. After calling this function, the caller should call
   * prepareEnumeration to get a new Enumeration instance.
   */
  void restartEnumeration() {
    enumeration_.reset();
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
  std::shared_ptr<Enumeration> enumeration_;
};
} // namespace facebook::eden
