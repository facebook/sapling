/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <optional>

#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

#ifdef _WIN32
/** Compute the sha1 of the file */
Hash20 getFileSha1(AbsolutePathPiece filePath, bool windowsSymlinksEnabled);

/** Compute the blake3 of the file */
Hash32 getFileBlake3(
    AbsolutePathPiece filePath,
    const std::optional<std::string>& maybeBlake3Key,
    bool windowsSymlinksEnabled);
#endif

} // namespace facebook::eden
