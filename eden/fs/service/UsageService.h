/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/futures/Future.h>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace facebook::eden {

class UsageService {
 public:
  virtual ~UsageService() = default;

  /**
   * Queries a predictive service for the top N directories given a user and
   * repo name.
   *
   * Used for the predictiveGlobFiles Thrift method.
   */
  virtual folly::SemiFuture<std::vector<std::string>> getTopUsedDirs(
      std::string_view user,
      std::string_view repo,
      uint32_t numResults,
      std::optional<std::string_view> os,
      std::optional<uint64_t> startTime,
      std::optional<uint64_t> endTime,
      std::optional<std::string> scAlias) = 0;
};

class NullUsageService : public UsageService {
 public:
  folly::SemiFuture<std::vector<std::string>> getTopUsedDirs(
      std::string_view user,
      std::string_view repo,
      uint32_t numResults,
      std::optional<std::string_view> os,
      std::optional<uint64_t> startTime,
      std::optional<uint64_t> endTime,
      std::optional<std::string> scAlias) override;
};

} // namespace facebook::eden
