/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileUtils.h"
#include <fmt/format.h>
#include "folly/FileUtil.h"

#ifdef _WIN32
#include "eden/fs/utils/Handle.h"
#endif

namespace facebook {
namespace eden {

#ifndef _WIN32

folly::Try<std::string> readFile(AbsolutePathPiece path, size_t num_bytes) {
  std::string ret;

  if (!folly::readFile(path.stringPiece().data(), ret, num_bytes)) {
    return folly::Try<std::string>{folly::makeSystemError(
        fmt::format(FMT_STRING("couldn't read {}"), path))};
  }

  return folly::Try{ret};
}

folly::Try<void> writeFile(AbsolutePathPiece path, folly::ByteRange data) {
  if (!folly::writeFile(data, path.stringPiece().data())) {
    return folly::Try<void>{folly::makeSystemError(
        fmt::format(FMT_STRING("couldn't write {}"), path))};
  }

  return folly::Try<void>{};
}

folly::Try<void> writeFileAtomic(
    AbsolutePathPiece path,
    folly::ByteRange data) {
  iovec iov;
  iov.iov_base = const_cast<unsigned char*>(data.data());
  iov.iov_len = data.size();

  if (auto err = folly::writeFileAtomicNoThrow(path.stringPiece(), &iov, 1)) {
    return folly::Try<void>{folly::makeSystemErrorExplicit(
        err, fmt::format(FMT_STRING("couldn't update {}"), path))};
  }

  return folly::Try<void>{};
}

#else

off_t getMaterializedFileSize(struct stat& st, AbsolutePath& pathToFile) {
  struct stat targetStat;
  if (::stat(pathToFile.c_str(), &targetStat) == 0) {
    st.st_size = targetStat.st_size;
  }
  return st.st_size;
}

namespace {
/*
 * Following is a traits class for File System handles with its handle value and
 * close function.
 */
struct FileHandleTraits {
  using Type = HANDLE;

  static Type invalidHandleValue() noexcept {
    return INVALID_HANDLE_VALUE;
  }
  static void close(Type handle) noexcept {
    CloseHandle(handle);
  }
};

using FileHandle = HandleBase<FileHandleTraits>;

enum class OpenMode {
  READ,
  WRITE,
};

folly::Try<FileHandle> openHandle(AbsolutePathPiece path, OpenMode mode) {
  DWORD dwDesiredAccess;
  DWORD dwCreationDisposition;

  if (mode == OpenMode::READ) {
    dwDesiredAccess = GENERIC_READ;
    dwCreationDisposition = OPEN_EXISTING;
  } else {
    dwDesiredAccess = GENERIC_WRITE;
    dwCreationDisposition = CREATE_ALWAYS;
  }

  auto widePath = path.wide();
  FileHandle fileHandle{CreateFileW(
      widePath.c_str(),
      dwDesiredAccess,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      dwCreationDisposition,
      FILE_ATTRIBUTE_NORMAL,
      nullptr)};
  if (!fileHandle) {
    return folly::Try<FileHandle>{makeWin32ErrorExplicit(
        GetLastError(), fmt::format(FMT_STRING("couldn't open {}"), path))};
  } else {
    return folly::Try{std::move(fileHandle)};
  }
}

folly::Try<void> writeToHandle(
    FileHandle& handle,
    folly::ByteRange data,
    AbsolutePathPiece path) {
  // TODO(xavierd): This can only write up to 4GB.
  if (data.size() > std::numeric_limits<DWORD>::max()) {
    return folly::Try<void>{std::invalid_argument(fmt::format(
        FMT_STRING("files over 4GB can't be written to, size={}"),
        data.size()))};
  }

  DWORD written = 0;
  if (!WriteFile(
          handle.get(),
          data.data(),
          folly::to_narrow(data.size()),
          &written,
          nullptr)) {
    return folly::Try<void>{makeWin32ErrorExplicit(
        GetLastError(), fmt::format(FMT_STRING("couldn't write {}"), path))};
  }

  return folly::Try<void>{};
}

} // namespace

folly::Try<std::string> readFile(AbsolutePathPiece path, size_t num_bytes) {
  auto tryFileHandle = openHandle(path, OpenMode::READ);
  if (tryFileHandle.hasException()) {
    return folly::Try<std::string>{std::move(tryFileHandle).exception()};
  }
  auto fileHandle = std::move(tryFileHandle).value();

  if (num_bytes == std::numeric_limits<size_t>::max()) {
    LARGE_INTEGER fileSize;
    if (!GetFileSizeEx(fileHandle.get(), &fileSize)) {
      return folly::Try<std::string>{makeWin32ErrorExplicit(
          GetLastError(),
          fmt::format(
              FMT_STRING("couldn't obtain the file size of {}"), path))};
    }
    num_bytes = fileSize.QuadPart;
  }

  // TODO(xavierd): this can only read up to 4GB.
  if (num_bytes > std::numeric_limits<DWORD>::max()) {
    return folly::Try<std::string>{std::invalid_argument(fmt::format(
        FMT_STRING("files over 4GB can't be read, filesize={}"), num_bytes))};
  }

  std::string ret(num_bytes, 0);
  DWORD read = 0;
  if (!ReadFile(
          fileHandle.get(),
          ret.data(),
          folly::to_narrow(num_bytes),
          &read,
          nullptr)) {
    return folly::Try<std::string>{makeWin32ErrorExplicit(
        GetLastError(), fmt::format(FMT_STRING("couldn't read {}"), path))};
  }

  return folly::Try{ret};
}

folly::Try<void> writeFile(AbsolutePathPiece path, folly::ByteRange data) {
  auto tryFileHandle = openHandle(path, OpenMode::WRITE);
  if (tryFileHandle.hasException()) {
    return folly::Try<void>{std::move(tryFileHandle).exception()};
  }

  return writeToHandle(tryFileHandle.value(), data, path);
}

folly::Try<void> writeFileAtomic(
    AbsolutePathPiece path,
    folly::ByteRange data) {
  auto parent = path.dirname();
  wchar_t tmpFile[MAX_PATH];

  if (GetTempFileNameW(parent.wide().c_str(), L"tmp", 0, tmpFile) == 0) {
    auto err = GetLastError();
    return folly::Try<void>{makeWin32ErrorExplicit(
        err,
        fmt::format(
            FMT_STRING("couldn't create a temporary file for {}"), path))};
  }

  auto tryTmpFileWrite = writeFile(AbsolutePath(tmpFile), data);
  if (tryTmpFileWrite.hasException()) {
    return tryTmpFileWrite;
  }

  if (!MoveFileExW(tmpFile, path.wide().c_str(), MOVEFILE_REPLACE_EXISTING)) {
    auto err = GetLastError();
    return folly::Try<void>{makeWin32ErrorExplicit(
        err, fmt::format(FMT_STRING("couldn't replace {}"), path))};
  }

  return folly::Try<void>{};
}

#endif

} // namespace eden
} // namespace facebook
