/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/UsageService.h"
#include <folly/logging/xlog.h>

namespace facebook::eden {

folly::SemiFuture<std::vector<std::string>> NullUsageService::getTopUsedDirs(
    std::string_view /*user*/,
    std::string_view /*repo*/,
    uint32_t /*numResults*/,
    std::optional<std::string_view> /*os*/,
    std::optional<uint64_t> /*startTime*/,
    std::optional<uint64_t> /*endTime*/,
    std::optional<std::string> /*scAlias*/) {
  XLOG_EVERY_MS(WARN, 60000)
      << "getTopUsedDirs not supported - returning empty directory list";
  return std::vector<std::string>{};
}

} // namespace facebook::eden
