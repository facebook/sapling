/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#if defined(__linux__) || defined(__APPLE__)

#include <chrono>
#include <string>

#include <folly/CancellationToken.h>
#include <folly/Expected.h>

#include "eden/fs/privhelper/PinScan.h"
#include "eden/fs/telemetry/LogEvent.h"

namespace facebook::eden {

/**
 * Run `<helperPath> --scan-pins` (the privhelper's one-shot mode) to discover
 * inodes pinned by processes on this user's EdenFS mounts: directories used
 * as cwds/roots and, on macOS, files held open or mapped.
 *
 * The scan runs as a separate process with a hard deadline: it inspects
 * every process, which in pathological cases can touch unrelated wedged
 * filesystems, and killing an overrunning child must not affect the daemon. The
 * wait also ends as soon as cancellation is requested, so a GC being stopped
 * for shutdown or checkout does not sit behind the scan.
 *
 * Returns the report, or a PinScanFailure saying why there is none (a
 * failed or timed-out helper, unparsable output, ...); pressure GC must then
 * treat pins as unknown and skip directory invalidation. A scan ended by
 * cancellation is reported with reason "cancelled", which is not a failure
 * worth logging.
 */
folly::Expected<PinScanReport, PinScanFailure> runPinScan(
    const std::string& helperPath,
    const folly::CancellationToken& cancellationToken,
    std::chrono::milliseconds timeout = std::chrono::seconds{10});

} // namespace facebook::eden

#endif // __linux__ || __APPLE__
