/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

namespace facebook::eden {

class ServerState;
class UnboundedQueueExecutor;

std::shared_ptr<ServerState> createTestServerState(
    std::shared_ptr<UnboundedQueueExecutor> fsChannelThreadPool = nullptr);

} // namespace facebook::eden
