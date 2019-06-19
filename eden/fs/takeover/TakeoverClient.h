/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook {
namespace eden {

/**
 * Request to take over mount points from an existing edenfs process.
 *
 * Returns a TakeoverData object on success, or throws an exception on error.
 */
TakeoverData takeoverMounts(
    AbsolutePathPiece socketPath,
    // this parameter is present for testing purposes and should not normally
    // be used in the production build.
    const std::set<int32_t>& supportedTakeoverVersions =
        kSupportedTakeoverVersions);

} // namespace eden
} // namespace facebook
