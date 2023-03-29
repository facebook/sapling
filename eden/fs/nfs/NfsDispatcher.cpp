/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/NfsDispatcher.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {

NfsDispatcher::NfsDispatcher(EdenStatsPtr stats, const Clock& clock)
    : stats_{std::move(stats)}, clock_{clock} {}

NfsDispatcher::~NfsDispatcher() = default;

} // namespace facebook::eden
