/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/Subprocess.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>

namespace facebook {
namespace eden {

/** Given a Subprocess instance, returns a SemiFuture that will yield its
 * resultant ProcessReturnCode when the process completes.
 * The SemiFuture is implemented by polling the return code at the specified
 * poll_interval (default is 10ms).
 * The polling is managed by a timer registered with the global IO Executor.
 */
folly::SemiFuture<folly::ProcessReturnCode> futureSubprocess(
    folly::Subprocess proc,
    std::chrono::milliseconds poll_interval = std::chrono::milliseconds(10));
} // namespace eden
} // namespace facebook
