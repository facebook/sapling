/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef __linux__

#include <chrono>
#include <optional>
#include <string>

#include <folly/CancellationToken.h>

#include "eden/fs/privhelper/PinScan.h"

namespace facebook::eden {

/**
 * Run `<helperPath> --scan-pins` (the privhelper's one-shot mode) to discover
 * directories pinned as process cwds/roots on this user's EdenFS mounts.
 *
 * The scan runs as a separate process with a hard deadline: it stats every
 * process's /proc magic links, which in pathological cases can touch
 * unrelated wedged filesystems, and killing an overrunning child must not
 * affect the daemon. The wait also ends as soon as cancellation is requested,
 * so a GC being stopped for shutdown or checkout does not sit behind the
 * scan.
 *
 * Returns std::nullopt if the scan did not complete successfully (failure,
 * timeout, cancellation, or unparsable output); pressure GC must then treat
 * pins as unknown and skip directory invalidation.
 */
std::optional<PinScanReport> runPinScan(
    const std::string& helperPath,
    const folly::CancellationToken& cancellationToken,
    std::chrono::milliseconds timeout = std::chrono::seconds{10});

} // namespace facebook::eden

#endif // __linux__
