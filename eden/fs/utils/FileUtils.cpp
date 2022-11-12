/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileUtils.h"
#include <boost/filesystem.hpp>
#include <fmt/format.h>

#include <folly/Exception.h>
#include <folly/FileUtil.h>

namespace facebook::eden {

#ifndef _WIN32

folly::Try<std::string> readFile(AbsolutePathPiece path, size_t num_bytes) {
  std::string ret;

  if (!folly::readFile(path.asString().c_str(), ret, num_bytes)) {
    return folly::Try<std::string>{folly::makeSystemError(
        fmt::format(FMT_STRING("couldn't read {}"), path))};
  }

  return folly::Try{std::move(ret)};
}

folly::Try<void> writeFile(AbsolutePathPiece path, folly::ByteRange data) {
  if (!folly::writeFile(data, path.asString().c_str())) {
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

folly::Try<std::vector<PathComponent>> getAllDirectoryEntryNames(
    AbsolutePathPiece path) {
  auto boostPath = boost::filesystem::path(path.stringPiece());
  std::vector<PathComponent> direntNames;

  boost::system::error_code ec;
  auto iter = boost::filesystem::directory_iterator(boostPath, ec);
  if (ec) {
    return folly::Try<std::vector<PathComponent>>{std::system_error(
        ec, fmt::format(FMT_STRING("couldn't iterate {}"), path))};
  }

  for (const auto& entry : iter) {
    direntNames.emplace_back(entry.path().filename().c_str());
  }
  return folly::Try{std::move(direntNames)};
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
      FILE_ATTRIBUTE_NORMAL | FILE_FLAG_BACKUP_SEMANTICS,
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

  return folly::Try{std::move(ret)};
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

namespace {
/*
 * None of the following are present in the SDK, thus we have to define them by
 * hand. Some were slightly modified from MSDN to limit the amount of data that
 * needed to be manually defined.
 */

typedef LONG NTSTATUS;
constexpr NTSTATUS STATUS_NO_MORE_FILES = 0x80000006L;

typedef struct _FILE_NAMES_INFORMATION {
  ULONG NextEntryOffset;
  ULONG FileIndex;
  ULONG FileNameLength;
  WCHAR FileName[1];
} FILE_NAMES_INFORMATION, *PFILE_NAMES_INFORMATION;

typedef struct _IO_STATUS_BLOCK {
  union {
    NTSTATUS Status;
    PVOID Pointer;
  };
  ULONG_PTR Information;
} IO_STATUS_BLOCK, *PIO_STATUS_BLOCK;

typedef enum _FILE_INFORMATION_CLASS {
  FileNamesInformation = 12, // 12
} FILE_INFORMATION_CLASS,
    *PFILE_INFORMATION_CLASS;

typedef NTSTATUS (*__kernel_entry NtQueryDirectoryFileP)(
    HANDLE FileHandle,
    HANDLE Event,
    PVOID ApcRoutine,
    PVOID ApcContext,
    PIO_STATUS_BLOCK IoStatusBlock,
    PVOID FileInformation,
    ULONG Length,
    FILE_INFORMATION_CLASS FileInformationClass,
    BOOLEAN ReturnSingleEntry,
    PVOID FileName,
    BOOLEAN RestartScan);

NTSTATUS NtQueryDirectoryFileImpl(
    const FileHandle& handle,
    void* buffer,
    size_t bufferSize) {
  static HMODULE ntdll = GetModuleHandleW(L"Ntdll.dll");
  static NtQueryDirectoryFileP impl = reinterpret_cast<NtQueryDirectoryFileP>(
      GetProcAddress(ntdll, "NtQueryDirectoryFile"));

  IO_STATUS_BLOCK iosb;
  return impl(
      handle.get(),
      nullptr,
      nullptr,
      nullptr,
      &iosb,
      buffer,
      bufferSize,
      FileNamesInformation,
      false,
      nullptr,
      false);
}

} // namespace

folly::Try<std::vector<PathComponent>> getAllDirectoryEntryNames(
    AbsolutePathPiece path) {
  auto handleTry = openHandle(path, OpenMode::READ);
  if (handleTry.hasException()) {
    return folly::Try<std::vector<PathComponent>>{
        std::move(handleTry).exception()};
  }
  const auto& handle = handleTry.value();

  std::vector<PathComponent> direntNames;
  while (true) {
    // The buffer must be 4 bytes aligned as described in
    // https://docs.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_names_information
    alignas(4) char buffer[16 * 1024];
    auto status = NtQueryDirectoryFileImpl(handle, &buffer, sizeof(buffer));
    if (status != 0) {
      if (status == STATUS_NO_MORE_FILES) {
        return folly::Try{std::move(direntNames)};
      }

      return folly::Try<std::vector<PathComponent>>{makeHResultErrorExplicit(
          HRESULT_FROM_NT(status),
          fmt::format(
              FMT_STRING("couldn't iterate on {}, {:x}"),
              path,
              (uint32_t)status))};
    }

    FILE_NAMES_INFORMATION* dirent =
        reinterpret_cast<FILE_NAMES_INFORMATION*>(&buffer);
    while (dirent != nullptr) {
      auto win_name = std::wstring_view{
          dirent->FileName,
          dirent->FileNameLength / sizeof(dirent->FileName[0])};
      if (win_name != L"." && win_name != L"..") {
        direntNames.emplace_back(win_name);
      }

      if (dirent->NextEntryOffset == 0) {
        dirent = nullptr;
      } else {
        dirent = reinterpret_cast<FILE_NAMES_INFORMATION*>(
            reinterpret_cast<char*>(dirent) + dirent->NextEntryOffset);
      }
    }
  }
}

#endif

} // namespace facebook::eden
