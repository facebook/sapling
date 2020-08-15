/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <string>
#include <vector>
#include "eden/fs/model/Hash.h"

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

  //
  // File size. For directories it will ignored
  //
  size_t size{0};

  FileMetadata(std::wstring&& name, bool isDir, size_t size)
      : name(std::move(name)), isDirectory(isDir), size(size) {}

  FileMetadata() = delete;
};

class Enumerator {
 public:
  Enumerator(const Enumerator&) = delete;
  Enumerator& operator=(const Enumerator&) = delete;

  Enumerator(std::vector<FileMetadata>&& entryList);

  Enumerator(Enumerator&& other)
      : metadataList_(std::move(other.metadataList_)),
        searchExpression_(std::move(other.searchExpression_)),
        listIndex_(std::move(other.listIndex_)) {}

  explicit Enumerator() = delete;

  const FileMetadata* current();

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
