/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Range.h>
#include <folly/Try.h>
#include <limits>
#include <string>
#include "eden/common/utils/Handle.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

#ifdef _WIN32
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
#endif

/** Read up to num_bytes bytes from the file */
FOLLY_NODISCARD folly::Try<std::string> readFile(
    AbsolutePathPiece path,
    size_t num_bytes = std::numeric_limits<size_t>::max());

/** Write data to the file pointed by path */
FOLLY_NODISCARD folly::Try<void> writeFile(
    AbsolutePathPiece path,
    folly::ByteRange data);

/** Atomically replace the content of the file with data.
 *
 * On failure, the content of the file is unchanged.
 */
FOLLY_NODISCARD folly::Try<void> writeFileAtomic(
    AbsolutePathPiece path,
    folly::ByteRange data);

/**
 * Read all the directory entry and return their names.
 *
 * On non-Windows OS, this is simply a wrapper around
 * boost::filesystem::directory_iterator.
 *
 * On Windows, we have to use something different as Boost will use the
 * FindFirstFile API which doesn't allow the directory to be opened with
 * FILE_SHARE_DELETE. This sharing flags allows the directory to be
 * renamed/deleted while it is being iterated on.
 */
FOLLY_NODISCARD folly::Try<std::vector<PathComponent>>
getAllDirectoryEntryNames(AbsolutePathPiece path);

#ifdef _WIN32
/**
 * For Windows only, returns the file size of the materialized file.
 */
off_t getMaterializedFileSize(struct stat& st, AbsolutePath& pathToFile);
#endif // _WIN32

} // namespace facebook::eden
