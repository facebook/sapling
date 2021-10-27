/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

namespace facebook {
namespace eden {

struct FileMetadata {
  //
  // File name : final component
  //
  std::wstring name;

  //
  // isDirectory will be set only for the directories
  // For files it will be ignored
  //
  bool isDirectory{false};

  folly::Future<uint64_t> getSize() {
    return sizeFuture_.getFuture();
  }

  FileMetadata(
      std::wstring&& name,
      bool isDir,
      ImmediateFuture<uint64_t> sizeFuture)
      : name(std::move(name)),
        isDirectory(isDir),
        // In the case where the future isn't ready yet, we want to start
        // driving it immediately, thus convert it to a Future.
        sizeFuture_(std::move(sizeFuture)
                        .semi()
                        .via(&folly::QueuedImmediateExecutor::instance())) {}

  FileMetadata() = delete;

 private:
  folly::FutureSplitter<uint64_t> sizeFuture_;
};

class Enumerator {
 public:
  Enumerator(const Enumerator&) = delete;
  Enumerator& operator=(const Enumerator&) = delete;

  Enumerator(std::vector<FileMetadata>&& entryList);

  Enumerator(Enumerator&& other) noexcept
      : searchExpression_(std::move(other.searchExpression_)),
        metadataList_(std::move(other.metadataList_)),
        listIndex_(std::move(other.listIndex_)) {}

  explicit Enumerator() = delete;

  FileMetadata* current();

  void advance() {
    ++listIndex_;
  }

  void restart() {
    listIndex_ = 0;
  }

  bool isSearchExpressionEmpty() const {
    return searchExpression_.empty();
  }

  void saveExpression(std::wstring searchExpression) noexcept {
    searchExpression_ = std::move(searchExpression);
  }

 private:
  std::wstring searchExpression_;
  std::vector<FileMetadata> metadataList_;

  //
  // use the listIndex_ to return entries when the enumeration is done over
  // multiple calls
  //
  size_t listIndex_ = 0;
};
} // namespace eden
} // namespace facebook
