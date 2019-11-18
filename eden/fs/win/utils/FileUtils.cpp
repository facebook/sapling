/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include <folly/portability/IOVec.h>
#include <filesystem>
#include <iostream>
#include "FileUtils.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

using folly::ByteRange;
using folly::MutableByteRange;

namespace facebook {
namespace eden {

DWORD readFile(HANDLE handle, void* buffer, DWORD bytesToRead) {
  DWORD bytesRead = 0;
  // According to MSDN, ReadFile for FS will not return until it has read all
  // the requested bytes or reached EOF. So we don't need to read it in a loop.

  if (!ReadFile(handle, buffer, bytesToRead, &bytesRead, nullptr)) {
    DWORD error = GetLastError();
    XLOGF(
        ERR,
        "Error while reading : bytesRead {}, Error: {} : {}",
        bytesRead,
        error,
        win32ErrorToString(error));

    throw makeWin32ErrorExplicit(error, "Error while reading");
  }
  return bytesRead;
}

DWORD writeFile(HANDLE handle, const void* buffer, DWORD bytesToWrite) {
  DWORD bytesWritten = 0;
  // According to MSDN, WriteFile for FS will not return until it has written
  // all the requested bytes. So we don't need to write it in a loop.

  if (!WriteFile(handle, buffer, bytesToWrite, &bytesWritten, nullptr)) {
    DWORD error = GetLastError();
    XLOGF(
        ERR,
        "Error while writing: bytesWritten {}, {} : {}",
        bytesWritten,
        error,
        win32ErrorToString(error));

    throw makeWin32ErrorExplicit(error, "Error while writing");
  }

  return bytesWritten;
}

DWORD writeFileIov(HANDLE handle, const iovec* iov, int count) {
  DWORD bytesWritten = 0;
  DWORD written = 0;

  for (int i = 0; i < count; i++) {
    written = writeFile(handle, iov[i].iov_base, iov[i].iov_len);
    bytesWritten += written;
  }

  return bytesWritten;
}

void writeFile(const wchar_t* filePath, const folly::ByteRange data) {
  static_assert(
      sizeof(data[0]) == 1,
      "writeFile: only containers with byte-sized elements accepted");

  FileHandle fileHandle{CreateFile(
      filePath,
      GENERIC_WRITE,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      CREATE_ALWAYS,
      FILE_ATTRIBUTE_NORMAL,
      nullptr)};

  if (!fileHandle) {
    throw makeWin32ErrorExplicit(
        GetLastError(),
        folly::sformat(
            "Unable to create the file {}", wideToMultibyteString(filePath)));
  }

  if (!data.empty()) {
    DWORD bytesWritten = writeFile(fileHandle.get(), data.data(), data.size());
    if (bytesWritten != data.size()) {
      throw std::logic_error(folly::sformat(
          "Partial data written, size {}, written {}",
          data.size(),
          bytesWritten));
    }
  }
}

void writeFileAtomic(const wchar_t* filePath, const folly::ByteRange data) {
  std::filesystem::path fullPath{filePath};
  auto parent = fullPath.parent_path();
  std::wstring tmpFile(MAX_PATH, 0);

  DWORD retVal = GetTempFileName(parent.c_str(), L"TMP_", 0, tmpFile.data());

  if (retVal == 0) {
    throw makeWin32ErrorExplicit(
        GetLastError(),
        folly::sformat(
            "Unable to get the temp file name: {}",
            wideToMultibyteString(filePath)));
  }

  writeFile(tmpFile.c_str(), data);

  if (!MoveFileEx(tmpFile.c_str(), filePath, MOVEFILE_REPLACE_EXISTING)) {
    auto error = GetLastError();
    DeleteFile(tmpFile.c_str());
    throw makeWin32ErrorExplicit(
        error,
        folly::sformat(
            "Unable to move the file: {}", wideToMultibyteString(filePath)));
  }
}

Hash getFileSha1(const wchar_t* filePath) {
  std::string data;
  readFile(filePath, data);
  return Hash::sha1(folly::StringPiece{data.c_str()});
}

struct EnumerationHandleTraits {
  using Type = HANDLE;

  static Type invalidHandleValue() noexcept {
    return INVALID_HANDLE_VALUE;
  }
  static void close(Type handle) noexcept {
    FindClose(handle);
  }
};

using EnumerationHandle = HandleBase<EnumerationHandleTraits>;

std::vector<DirectoryEntryW> getEnumerationEntries(
    const std::wstring& dirPath) {
  std::vector<DirectoryEntryW> entries;

  DirectoryEntryW entry;
  EnumerationHandle enumHandle{FindFirstFileW(dirPath.c_str(), &entry.data)};
  if (!enumHandle) {
    DWORD error = GetLastError();
    if (error == ERROR_NO_MORE_FILES) {
      return entries;
    }
    throw makeWin32ErrorExplicit(
        error,
        folly::sformat("Enumeration failed for: ", winToEdenPath(dirPath)));
  }

  do {
    if ((std::wstring_view(entry.data.cFileName) != std::wstring_view(L".")) &&
        (std::wstring_view(entry.data.cFileName) != std::wstring_view(L".."))) {
      entries.emplace_back(entry);
    }
  } while (FindNextFileW(enumHandle.get(), &entry.data));

  DWORD error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw makeWin32ErrorExplicit(
        error,
        folly::sformat(
            "Failed to get enumeration entries for: {}",
            winToEdenPath(dirPath)));
  }
  return entries;
}

std::vector<DirectoryEntryA> getEnumerationEntries(const std::string& dirPath) {
  std::vector<DirectoryEntryA> entries;

  DirectoryEntryA entry;
  EnumerationHandle enumHandle{FindFirstFileA(dirPath.c_str(), &entry.data)};
  if (!enumHandle) {
    DWORD error = GetLastError();
    if (error == ERROR_NO_MORE_FILES) {
      return entries;
    }
    throw makeWin32ErrorExplicit(
        error, folly::sformat("Enumeration failed for: ", dirPath));
  }

  do {
    if ((folly::StringPiece(entry.data.cFileName) != folly::StringPiece(".")) &&
        (folly::StringPiece(entry.data.cFileName) !=
         folly::StringPiece(".."))) {
      entries.emplace_back(entry);
    }
  } while (FindNextFileA(enumHandle.get(), &entry.data));

  DWORD error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw makeWin32ErrorExplicit(
        error,
        folly::sformat("Failed to get enumeration entries for: ", dirPath));
  }
  return entries;
}

} // namespace eden
} // namespace facebook
