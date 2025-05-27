/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/takeover/TakeoverData.h"

namespace facebook::eden {

folly::Future<UnixSocket::Message> receiveTakeoverDataMessage(
    FutureUnixSocket& socket,
    UnixSocket::Message&& msg,
    const std::chrono::seconds& takeoverReceiveTimeout);

/**
 * Request to take over mount points from an existing edenfs process.
 *
 * Returns a TakeoverData object on success, or throws an exception on error.
 */
TakeoverData takeoverMounts(
    AbsolutePathPiece socketPath,
    const std::chrono::seconds& takeoverReceiveTimeout,
    // the following parameters are present for testing purposes and should not
    // normally be used in the production build.
    bool shouldThrowDuringTakeover = false,
    bool shouldPing = true,
    const std::set<int32_t>& supportedTakeoverVersions =
        kSupportedTakeoverVersions,
    const uint64_t supportedTakeoverCapabilities = kSupportedCapabilities);

} // namespace facebook::eden
