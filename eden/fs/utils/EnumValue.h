/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Utility.h>

namespace facebook {
namespace eden {

/**
 * It's common in error messages to log the underlying value of an enumeration.
 * Bring a short function into the eden namespace to retrieve that value.
 */
template <typename E>
constexpr std::underlying_type_t<E> enumValue(E e) noexcept {
  return folly::to_underlying(e);
}

} // namespace eden
} // namespace facebook
