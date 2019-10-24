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
#include <filesystem>
#include <memory>
#include <string>
#include "eden/fs/win/utils/WinError.h"

namespace facebook {
namespace eden {

//
// Defining the wide char path pointer and strings. On Windows where we need
// wide char paths we will use std::filesystem::path, until we port PathFunc to
// work with widechar strings. std::filesystem::path has member functions to
// internally convert path from wide char to multibyte and also handle the
// backslash and forward slash conversion, so it will be the best choice for our
// case.
//
// We don't have anything that would sanity check the paths to be relative or
// absolute and a function which expects an Absolute path will not complain if
// relative is passed.
//

using ConstWinRelativePathWPtr = const wchar_t*;
using ConstWinAbsolutePathWPtr = const wchar_t*;
using WinRelativePathW = std::filesystem::path;
using WinAbsolutePathW = std::filesystem::path;
using WinPathComponentW = std::wstring;

// TODO: Move these functions to the better location.

// The paths we receive from FS and cli are Windows paths (Win path separator
// and UTF16). For now there will be two separate areas in our Windows code one
// which will use Windows strings and the other with (UTF8 + Unix path
// separator). The functions in stringconv will be responsible to do the
// conversion.

static std::string wcharToString(const wchar_t* wideCString) {
  //
  // Return empty string if wideCString is nullptr or an empty string. Empty
  // string is a common scenario. All the FS ops for the root with
  // come with relative path as empty string.
  //
  if ((!wideCString) || (wideCString[0] == L'\0')) {
    return "";
  }

  // To avoid extra copy or using max size buffers we should get the size first
  // and allocate the right size buffer.
  int size = WideCharToMultiByte(CP_UTF8, 0, wideCString, -1, nullptr, 0, 0, 0);

  if (size > 0) {
    std::string multiByteString(size - 1, 0);
    size = WideCharToMultiByte(
        CP_UTF8, 0, wideCString, -1, multiByteString.data(), size, 0, 0);
    if (size > 0) {
      return multiByteString;
    }
  }
  throw makeWin32ErrorExplicit(
      GetLastError(), "Failed to convert wide char to char");
}

static std::wstring charToWstring(const char* multiByteCString) {
  //
  // Return empty string if multiByteCString is nullptr or an empty string.
  // Empty string is a common scenario. All the FS ops for the root
  // with come with relative path as empty string.
  //
  if ((!multiByteCString) || (multiByteCString[0] == '\0')) {
    return L"";
  }

  // To avoid extra copy or using max size buffers we should get the size first
  // and allocate the right size buffer.
  int size = MultiByteToWideChar(CP_UTF8, 0, multiByteCString, -1, nullptr, 0);

  if (size > 0) {
    std::wstring wideString(size - 1, 0);
    size = MultiByteToWideChar(
        CP_UTF8, 0, multiByteCString, -1, wideString.data(), size);
    if (size > 0) {
      return wideString;
    }
  }
  throw makeWin32ErrorExplicit(
      GetLastError(), "Failed to convert char to wide char");
}

static std::string wstringToString(const std::wstring& wideString) {
  return wcharToString(wideString.c_str());
}

static std::wstring stringToWstring(const std::string& multiByteString) {
  return charToWstring(multiByteString.c_str());
}

static std::string winToEdenPath(const std::wstring& winString) {
  std::string edenStr = wstringToString(winString);
#ifndef USE_WIN_PATH_SEPERATOR
  std::replace(edenStr.begin(), edenStr.end(), '\\', '/');
#endif
  return edenStr;
}

static std::wstring edenToWinPath(const std::string& edenString) {
  std::wstring winStr = stringToWstring(edenString);
#ifndef USE_WIN_PATH_SEPERATOR
  std::replace(winStr.begin(), winStr.end(), L'/', L'\\');
#endif
  return winStr;
}

static std::string winToEdenName(const std::wstring& wideName) {
  //
  // This function is to convert the final name component of the path
  // which should not contain the path delimiter. Assert that.
  //
  assert(wideName.find(L'\\') == std::wstring::npos);
  return wstringToString(wideName.c_str());
}

static std::wstring edenToWinName(const std::string& name) {
  //
  // This function is to convert the final name component of the path
  // which should not contain the path delimiter. Assert that.
  //
  assert(name.find('/') == std::string::npos);
  return stringToWstring(name.c_str());
}

} // namespace eden
} // namespace facebook
