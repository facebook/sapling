/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

namespace facebook::eden {

template <typename T, size_t Expected, size_t Actual = sizeof(T)>
constexpr bool CheckSize() {
  static_assert(Expected == Actual);
  return true;
}

template <size_t Expected, size_t Actual>
constexpr bool CheckEqual() {
  static_assert(Expected == Actual);
  return true;
}

} // namespace facebook::eden
