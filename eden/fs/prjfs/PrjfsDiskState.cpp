/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/prjfs/PrjfsDiskState.h"

#ifdef _WIN32
#include <folly/executors/SerialExecutor.h>
#include <folly/portability/Windows.h>

#include <winioctl.h> // @manual

#include "eden/common/utils/CaseSensitivity.h"
#include "eden/common/utils/DirType.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/windows/WinError.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/fs/utils/ProjfsUtil.h"

namespace facebook::eden {
namespace {

// Reparse tag for UNIX domain socket is not defined in Windows header files.
const ULONG IO_REPARSE_TAG_SOCKET = 0x80000023;

dtype_t dtypeFromAttrs(DWORD dwFileAttributes, DWORD dwReserved0) {
  if (dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) {
    // Microsoft documents the dwReserved0 member as holding the reparse tag:
    // https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-win32_find_dataw
    if (dwReserved0 == IO_REPARSE_TAG_SYMLINK ||
        dwReserved0 == IO_REPARSE_TAG_MOUNT_POINT) {
      return dtype_t::Symlink;
    } else if (dwReserved0 == IO_REPARSE_TAG_SOCKET) {
      return dtype_t::Socket;
    }

    // We don't care about other reparse point types, so treating them as
    // regular files/directories.

    if (dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
      return dtype_t::Dir;
    } else {
      return dtype_t::Regular;
    }
  } else if (dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
    return dtype_t::Dir;
  } else {
    return dtype_t::Regular;
  }
}

bool directoryIsEmpty(const wchar_t* path) {
  WIN32_FIND_DATAW findFileData;
  HANDLE h = FindFirstFileExW(
      path, FindExInfoBasic, &findFileData, FindExSearchNameMatch, nullptr, 0);
  if (h == INVALID_HANDLE_VALUE) {
    throw std::runtime_error(fmt::format(
        "unable to check directory - {}",
        wideToMultibyteString<std::string>(path)));
  }

  do {
    if (wcscmp(findFileData.cFileName, L".") == 0 ||
        wcscmp(findFileData.cFileName, L"..") == 0) {
      continue;
    }
    FindClose(h);
    return false;
  } while (FindNextFileW(h, &findFileData) != 0);

  auto error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw std::runtime_error(fmt::format(
        "unable to check directory - {}",
        wideToMultibyteString<std::string>(path)));
  }

  FindClose(h);
  return true;
}

void populateDiskState(
    AbsolutePathPiece root,
    RelativePathPiece path,
    FsckFileState& state,
    const WIN32_FIND_DATAW& findFileData,
    bool windowsSymlinksEnabled,
    bool fsckRenamedFiles) {
  dtype_t dtype =
      dtypeFromAttrs(findFileData.dwFileAttributes, findFileData.dwReserved0);
  if (dtype != dtype_t::Dir && dtype != dtype_t::Regular) {
    state.onDisk = true;
    // On Windows, EdenFS consider most special files (sockets, etc)
    // to be regular (but not symlinks)
    state.diskDtype = windowsSymlinksEnabled && dtype == dtype_t::Symlink
        ? dtype_t::Symlink
        : dtype_t::Regular;
    state.populatedOrFullOrTomb = true;
    return;
  }

  // Some empirical data on the values of reparse, recall, hidden, and
  // system dwFileAttributes, compared with the tombstone and full
  // getPrjFileState values.
  //
  // https://docs.microsoft.com/en-us/windows/win32/projfs/cache-state
  // https://docs.microsoft.com/en-us/windows/win32/fileio/file-attribute-constants
  //
  // clang-format off
  //
  // (reparse, recall, hidden, system) => (tomb,  materialized) dwFileAttributes
  // (false,   false,  false,  false)  => (false, true)  attr=16 (DIRECTORY)
  // (false,   false,  false,  false)  => (false, true)  attr=32 (ARCHIVE)
  // (true,    false,  true,   true)   => (true,  false) attr=1062 (REPARSE_POINT | ARCHIVE | HIDDEN | SYSTEM)
  // (true,    false,  false,  false)  => (false, false) attr=1568 (REPARSE_POINT | SPARSE_FILE | ARCHIVE)
  // (true,    true,   false,  false)  => (false, false) attr=4195344 (RECALL_ON_DATA_ACCESS | REPARSE_POINT | DIRECTORY)
  //
  // clang-format on
  // TODO: try to repro FILE_ATTRIBUTE_RECALL_ON_OPEN using a placeholder
  // directory
  auto reparse = (findFileData.dwFileAttributes &
                  FILE_ATTRIBUTE_REPARSE_POINT) == FILE_ATTRIBUTE_REPARSE_POINT;
  auto recall =
      (findFileData.dwFileAttributes & FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) ==
      FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS;
  auto hidden = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_HIDDEN) ==
      FILE_ATTRIBUTE_HIDDEN;
  auto system = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_SYSTEM) ==
      FILE_ATTRIBUTE_SYSTEM;
  auto sparse = (findFileData.dwFileAttributes & FILE_ATTRIBUTE_SPARSE_FILE) ==
      FILE_ATTRIBUTE_SPARSE_FILE;

  bool detectedTombstone = reparse && !recall && hidden && system;
  bool detectedFull = !reparse && !recall;

  state.onDisk = true;
  state.diskDtype = dtype;
  state.diskTombstone = detectedTombstone;

  // It can also be populated if a descendant directory is
  // materialized. But that is checked later when processing the children.
  state.populatedOrFullOrTomb = detectedFull || detectedTombstone;
  // It's an empty placeholder unless it's materialized or it has children.
  state.diskEmptyPlaceholder = !state.populatedOrFullOrTomb;
  state.directoryIsFull = !recall;

  state.renamedPlaceholder = false;

  if (fsckRenamedFiles && sparse) {
    auto renamedPlaceholderResult =
        isRenamedPlaceholder((root + path).wide().c_str());
    if (renamedPlaceholderResult.hasValue()) {
      state.renamedPlaceholder = renamedPlaceholderResult.value();
    } else {
      XLOGF(
          DBG9,
          "Error checking rename: {}",
          folly::exceptionStr(renamedPlaceholderResult.exception()));
    }
  }

  if (dtype == dtype_t::Dir) {
    auto absPath = root + path;
    auto wPath = absPath.wide();
    if (!directoryIsEmpty(wPath.c_str())) {
      state.diskEmptyPlaceholder = false;
    }
  }
}

} // namespace

/**
 * List all the on-disk entries and return a PathMap from them.
 */
PathMap<FsckFileState> getPrjfsOnDiskChildrenState(
    AbsolutePathPiece root,
    RelativePathPiece path,
    bool windowsSymlinksEnabled,
    bool fsckRenamedFiles,
    bool queryOnDiskEntriesOnly) {
  PathMap<FsckFileState> children{CaseSensitivity::Insensitive};
  auto absPath = (root + path + "*"_pc).wide();

  DWORD additionalFlags = 0;
  if (queryOnDiskEntriesOnly) {
    additionalFlags |= FIND_FIRST_EX_ON_DISK_ENTRIES_ONLY;
  }

  WIN32_FIND_DATAW findFileData;
  HANDLE h = FindFirstFileExW(
      absPath.c_str(),
      FindExInfoBasic,
      &findFileData,
      FindExSearchNameMatch,
      nullptr,
      additionalFlags);
  if (h == INVALID_HANDLE_VALUE) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }
  SCOPE_EXIT {
    FindClose(h);
  };

  do {
    if (wcscmp(findFileData.cFileName, L".") == 0 ||
        wcscmp(findFileData.cFileName, L"..") == 0 ||
        wcscmp(findFileData.cFileName, L".eden") == 0) {
      continue;
    }
    PathComponent name{findFileData.cFileName};
    auto& childState = children[name];
    populateDiskState(
        root,
        path + name,
        childState,
        findFileData,
        windowsSymlinksEnabled,
        fsckRenamedFiles);
  } while (FindNextFileW(h, &findFileData) != 0);

  auto error = GetLastError();
  if (error != ERROR_NO_MORE_FILES) {
    throw std::runtime_error(
        fmt::format("unable to iterate over directory - {}", path));
  }

  return children;
}

} // namespace facebook::eden

#endif // defined _WIN32
