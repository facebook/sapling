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
#include "folly/Range.h"
#include "folly/String.h"

namespace facebook {
namespace eden {

/**
 * Defining the wide char path pointer and strings. On Windows where we need
 * wide char paths we will use std::filesystem::path, until we port PathFunc to
 * work with widechar strings. std::filesystem::path has member functions to
 * internally convert path from wide char to multibyte and also handle the
 * backslash and forward slash conversion, so it will be the best choice for our
 * case.
 *
 * We don't have anything that would sanity check the paths to be relative or
 * absolute and a function which expects an Absolute path will not complain if
 * relative is passed.
 */

using ConstWinRelativePathWPtr = const wchar_t*;
using ConstWinAbsolutePathWPtr = const wchar_t*;
using WinRelativePathW = std::filesystem::path;
using WinAbsolutePathW = std::filesystem::path;
using WinPathComponentW = std::wstring;

// TODO: Move these functions to the better location.

/**
 * The paths we receive from FS and cli are Windows paths (Win path separator
 * and UTF16). For now there will be two separate areas in our Windows code one
 * which will use Windows strings and the other with (UTF8 + Unix path
 * separator). The functions in stringconv will be responsible to do the
 * conversion.
 */

// Helper for testing whether something quacks like a wide string
template <class T>
struct IsSomeWideString : std::false_type {};

template <>
struct IsSomeWideString<std::wstring> : std::true_type {};

template <>
struct IsSomeWideString<std::wstring_view> : std::true_type {};

// Following are for checking if it's a single char string. We are not using the
// one from Folly because that doesn't support std::string_view

template <class T>
struct IsSomeEdenString : std::false_type {};

template <>
struct IsSomeEdenString<std::string> : std::true_type {};

template <>
struct IsSomeEdenString<std::string_view> : std::true_type {};

template <>
struct IsSomeEdenString<folly::StringPiece> : std::true_type {};

template <class T>
struct IsStdPath : std::false_type {};

template <>
struct IsStdPath<std::filesystem::path> : std::true_type {};

/**
 * wideToMultibyteString can take a wide char container like wstring,
 * wstring_view and return a multibyte string as std::string.
 */
template <class T>
typename std::enable_if<IsSomeWideString<T>::value, std::string>::type
wideToMultibyteString(T const& wideCharPiece) {
  if (wideCharPiece.empty()) {
    return std::string{};
  }

  // To avoid extra copy or using max size buffers we should get the size first
  // and allocate the right size buffer.
  int size = WideCharToMultiByte(
      CP_UTF8, 0, wideCharPiece.data(), wideCharPiece.size(), nullptr, 0, 0, 0);

  if (size > 0) {
    std::string multiByteString(size, 0);
    size = WideCharToMultiByte(
        CP_UTF8,
        0,
        wideCharPiece.data(),
        wideCharPiece.size(),
        multiByteString.data(),
        multiByteString.size(),
        0,
        0);
    if (size == multiByteString.size()) {
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

template <class T>
typename std::enable_if<IsSomeEdenString<T>::value, std::wstring>::type
multibyteToWideString(T const& multiBytePiece) {
  if (multiBytePiece.empty()) {
    return L"";
  }

  // To avoid extra copy or using max size buffers we should get the size
  // first and allocate the right size buffer.
  int size = MultiByteToWideChar(
      CP_UTF8, 0, multiBytePiece.data(), multiBytePiece.size(), nullptr, 0);

  if (size > 0) {
    std::wstring wideString(size, 0);
    size = MultiByteToWideChar(
        CP_UTF8,
        0,
        multiBytePiece.data(),
        multiBytePiece.size(),
        wideString.data(),
        wideString.size());
    if (size == wideString.size()) {
      return wideString;
    }
  }
  throw makeWin32ErrorExplicit(
      GetLastError(), "Failed to convert char to wide char");
}

static std::string wideToMultibyteString(
    const wchar_t* FOLLY_NULLABLE wideCString) {
  //
  // Return empty string if wideCString is nullptr or an empty string. Empty
  // string is a common scenario. All the FS operations for the root have
  // relative path as empty string.
  //
  if (!wideCString) {
    return std::string{};
  }
  return wideToMultibyteString(std::wstring_view(wideCString));
}

static std::wstring multibyteToWideString(
    const char* FOLLY_NULLABLE multiByteCString) {
  //
  // Return empty string if multiByteCString is nullptr or an empty string.
  // Empty string is a common scenario. All the FS operations for the root have
  // relative path as empty string.
  //
  if (!multiByteCString) {
    return L"";
  }
  return multibyteToWideString(std::string_view(multiByteCString));
}

template <class T>
typename std::enable_if<IsSomeWideString<T>::value, std::string>::type
winToEdenPath(T const& winString) {
  std::string edenStr = wideToMultibyteString(winString);
#ifndef USE_WIN_PATH_SEPERATOR
  std::replace(edenStr.begin(), edenStr.end(), '\\', '/');
#endif
  return edenStr;
}

template <class T>
typename std::enable_if<IsStdPath<T>::value, std::string>::type winToEdenPath(
    T const& path) {
  return path.generic_string();
}

template <class T>
typename std::enable_if<IsSomeEdenString<T>::value, std::wstring>::type
edenToWinPath(T const& edenString) {
  std::wstring winStr = multibyteToWideString(edenString);
#ifndef USE_WIN_PATH_SEPERATOR
  std::replace(winStr.begin(), winStr.end(), L'/', L'\\');
#endif
  return winStr;
}

template <class T>
typename std::enable_if<IsSomeWideString<T>::value, std::string>::type
winToEdenName(T const& wideName) {
  //
  // This function is to convert the final name component of the path
  // which should not contain the path delimiter. Assert that.
  //
  assert(wideName.find(L'\\') == std::wstring::npos);
  return wideToMultibyteString(wideName);
}

template <class T>
typename std::enable_if<IsSomeEdenString<T>::value, std::wstring>::type
edenToWinName(T const& name) {
  //
  // This function is to convert the final name component of the path
  // which should not contain the path delimiter. Assert that.
  //
  assert(name.find('/') == std::string::npos);
  return multibyteToWideString(name);
}

} // namespace eden
} // namespace facebook
