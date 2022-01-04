/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/utils/StringConv.h"

namespace facebook::eden {

std::wstring multibyteToWideString(folly::StringPiece multiBytePiece) {
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

} // namespace facebook::eden

#endif
