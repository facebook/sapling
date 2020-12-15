/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>
#include <string>
#include <vector>
#include "eden/fs/model/Hash.h"
#include "folly/futures/Future.h"

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

  folly::Future<size_t> getSize() {
    if (cachedSize_) {
      return *cachedSize_;
    }

    return std::move(sizeFuture_).thenValue([this](size_t size) {
      cachedSize_ = size;
      return size;
    });
  }

  FileMetadata(
      std::wstring&& name,
      bool isDir,
      folly::Future<size_t> sizeFuture)
      : name(std::move(name)),
        isDirectory(isDir),
        sizeFuture_(std::move(sizeFuture)) {}

  FileMetadata() = delete;

 private:
  folly::Future<size_t> sizeFuture_;
  std::optional<size_t> cachedSize_;
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
