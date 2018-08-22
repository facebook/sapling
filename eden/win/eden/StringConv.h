/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#pragma once
#include "windows.h"

#include <memory>
#include <string>

namespace facebook {
namespace edenwin {

constexpr size_t strBufferLength = 2048;

// TODO: Move these functions to the better location.
class StringConv {
 public:
  static std::string wcharToString(const wchar_t* wideCString) {
    std::string multiByteString;
    auto buffer = std::make_unique<char[]>(strBufferLength);
    WideCharToMultiByte(
        CP_UTF8, 0, wideCString, -1, buffer.get(), strBufferLength, 0, 0);
    multiByteString = buffer.get();
    return multiByteString;
  }

  static std::wstring charToWstring(const char* multiByteCString) {
    std::wstring wideString;
    auto buffer = std::make_unique<wchar_t[]>(strBufferLength);
    MultiByteToWideChar(
        CP_UTF8, 0, multiByteCString, -1, buffer.get(), strBufferLength);
    wideString = buffer.get();
    return wideString;
  }

  static std::string wstringToString(const std::wstring& wideString) {
    return wcharToString(wideString.c_str());
  }

  static std::wstring stringToWstring(const std::string& multiByteString) {
    return charToWstring(multiByteString.c_str());
  }
};

} // namespace edenwin
} // namespace facebook
