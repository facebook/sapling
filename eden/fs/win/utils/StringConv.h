/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include "folly/portability/Windows.h"

#include <algorithm>
#include <cassert>
#include <memory>
#include <string>
#include "eden/fs/win/utils/WinError.h"
#include "folly/Range.h"
#include "folly/String.h"

namespace facebook {
namespace eden {

template <class MultiByteStringType>
MultiByteStringType wideToMultibyteString(std::wstring_view wideCharPiece) {
  if (wideCharPiece.empty()) {
    return MultiByteStringType{};
  }

  int inputSize = folly::to_narrow(folly::to_signed(wideCharPiece.size()));

  // To avoid extra copy or using max size buffers we should get the size first
  // and allocate the right size buffer.
  int size = WideCharToMultiByte(
      CP_UTF8, 0, wideCharPiece.data(), inputSize, nullptr, 0, 0, 0);

  if (size > 0) {
    MultiByteStringType multiByteString(size, 0);
    int resultSize = WideCharToMultiByte(
        CP_UTF8,
        0,
        wideCharPiece.data(),
        inputSize,
        multiByteString.data(),
        size,
        0,
        0);
    if (size == resultSize) {
      return multiByteString;
    }
  }
  throw makeWin32ErrorExplicit(
      GetLastError(), "Failed to convert wide char to char");
}

/**
 * multibyteToWideString can take a multibyte char container like string,
 * string_view, folly::StringPiece and return a widechar string in std::wstring.
 */

static std::wstring multibyteToWideString(folly::StringPiece multiBytePiece) {
  if (multiBytePiece.empty()) {
    return L"";
  }

  int inputSize = folly::to_narrow(folly::to_signed(multiBytePiece.size()));

  // To avoid extra copy or using max size buffers we should get the size
  // first and allocate the right size buffer.
  int size = MultiByteToWideChar(
      CP_UTF8, 0, multiBytePiece.data(), inputSize, nullptr, 0);

  if (size > 0) {
    std::wstring wideString(size, 0);
    int resultSize = MultiByteToWideChar(
        CP_UTF8, 0, multiBytePiece.data(), inputSize, wideString.data(), size);
    if (size == resultSize) {
      return wideString;
    }
  }
  throw makeWin32ErrorExplicit(
      GetLastError(), "Failed to convert char to wide char");
}

} // namespace eden
} // namespace facebook
