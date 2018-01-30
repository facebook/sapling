/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
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
