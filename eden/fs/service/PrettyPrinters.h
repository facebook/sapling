/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

/*
 * This file contains functions for helping format some of the types defined
 * in eden.thrift.
 *
 * This is primarily useful for unit tests and logging.
 */

#include "eden/fs/service/gen-cpp2/eden_types.h"

#include <fmt/format.h>

namespace facebook::eden {
void toAppend(const ConflictType& conflictType, std::string* result);
void toAppend(const CheckoutConflict& conflict, std::string* result);
void toAppend(const ScmFileStatus& scmFileStatus, std::string* result);
void toAppend(const MountState& mountState, std::string* result);
} // namespace facebook::eden

template <>
struct fmt::formatter<facebook::eden::ConflictType>
    : fmt::formatter<string_view> {
  template <typename FormatContext>
  auto format(
      const facebook::eden::ConflictType& conflictType,
      FormatContext& ctx) {
    // TODO: Avoid allocation here.
    return formatter<string_view>::format(
        folly::to<std::string>(conflictType), ctx);
  }
};

template <>
struct fmt::formatter<facebook::eden::CheckoutConflict>
    : fmt::formatter<string_view> {
  template <typename FormatContext>
  auto format(
      const facebook::eden::CheckoutConflict& conflict,
      FormatContext& ctx) {
    // TODO: Avoid allocation here.
    return formatter<string_view>::format(
        folly::to<std::string>(conflict), ctx);
  }
};

template <>
struct fmt::formatter<facebook::eden::ScmFileStatus>
    : fmt::formatter<string_view> {
  template <typename FormatContext>
  auto format(
      const facebook::eden::ScmFileStatus& scmFileStatus,
      FormatContext& ctx) {
    // TODO: Avoid allocation here.
    return formatter<string_view>::format(
        folly::to<std::string>(scmFileStatus), ctx);
  }
};

template <>
struct fmt::formatter<facebook::eden::MountState>
    : fmt::formatter<string_view> {
  template <typename FormatContext>
  auto format(
      const facebook::eden::MountState& mountState,
      FormatContext& ctx) {
    // TODO: Avoid allocation here.
    return formatter<string_view>::format(
        folly::to<std::string>(mountState), ctx);
  }
};
