/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/EdenStats.h"

#include <memory>

#include <fb303/ServiceData.h>
#include <fb303/ThreadCachedServiceData.h>

namespace facebook::eden {

void EdenStats::flush() {
  // Counters accumulate in thread-local cells that the ThreadCachedServiceData
  // publish thread drains periodically; durations are quantile stats that are
  // aggregated on read. A reader that needs this instant's values calls this.
  fb303::ThreadCachedServiceData::get()->publishStats();
  fb303::ServiceData::get()->getQuantileStatMap()->flushAll();
}

} // namespace facebook::eden
