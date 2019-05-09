/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include "folly/portability/Windows.h"

#include <string>
#include <vector>

namespace facebook {
namespace eden {

class WinStore;
struct FileMetadata;

class Enumerator {
 public:
  Enumerator(const Enumerator&) = delete;
  Enumerator& operator=(const Enumerator&) = delete;

  Enumerator(
      const GUID& enumerationId,
      const std::wstring& path,
      std::vector<FileMetadata> entryList);

  Enumerator() = delete;

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
  std::wstring path_;
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
