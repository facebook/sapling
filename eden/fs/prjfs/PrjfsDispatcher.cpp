/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/prjfs/PrjfsDispatcher.h"
#include "eden/fs/telemetry/EdenStats.h"

namespace facebook::eden {
PrjfsDispatcher::~PrjfsDispatcher() {}

PrjfsDispatcher::PrjfsDispatcher(EdenStatsPtr stats)
    : stats_{std::move(stats)} {}

const EdenStatsPtr& PrjfsDispatcher::getStats() const {
  return stats_;
}
} // namespace facebook::eden

#endif
