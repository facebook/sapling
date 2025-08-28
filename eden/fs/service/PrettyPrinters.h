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

namespace facebook::eden::detail {
template <typename ThriftEnum, typename OutputIt>
OutputIt formatThriftEnum(
    OutputIt out,
    const ThriftEnum& value,
    folly::StringPiece typeName) {
  const char* name = apache::thrift::TEnumTraits<ThriftEnum>::findName(value);
  if (name) {
    return fmt::format_to(out, "{}", name);
  } else {
    return fmt::format_to(out, "{}::{}", typeName, static_cast<int>(value));
  }
}
} // namespace facebook::eden::detail

template <>
struct fmt::formatter<facebook::eden::ConflictType> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(
      const facebook::eden::ConflictType& conflictType,
      FormatContext& ctx) const {
    return facebook::eden::detail::formatThriftEnum(
        ctx.out(), conflictType, "ConflictType");
  }
};

template <>
struct fmt::formatter<facebook::eden::CheckoutConflict> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(
      const facebook::eden::CheckoutConflict& conflict,
      FormatContext& ctx) const {
    auto out = ctx.out();
    out = fmt::format_to(out, "CheckoutConflict(type=");
    out = facebook::eden::detail::formatThriftEnum(
        out, *conflict.type(), "ConflictType");
    return fmt::format_to(
        out,
        ", path=\"{}\", message=\"{}\")",
        *conflict.path(),
        *conflict.message());
  }
};

template <>
struct fmt::formatter<facebook::eden::ScmFileStatus> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(
      const facebook::eden::ScmFileStatus& scmFileStatus,
      FormatContext& ctx) const {
    return facebook::eden::detail::formatThriftEnum(
        ctx.out(), scmFileStatus, "ScmFileStatus");
  }
};

template <>
struct fmt::formatter<facebook::eden::MountState> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename FormatContext>
  auto format(const facebook::eden::MountState& mountState, FormatContext& ctx)
      const {
    return facebook::eden::detail::formatThriftEnum(
        ctx.out(), mountState, "MountState");
  }
};
