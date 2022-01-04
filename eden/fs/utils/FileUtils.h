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
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

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

#ifdef _WIN32
/**
 * For Windows only, returns the file size of the materialized file.
 */
off_t getMaterializedFileSize(struct stat& st, AbsolutePath& pathToFile);
#endif // _WIN32

} // namespace eden
} // namespace facebook
