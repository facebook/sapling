/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Synchronized.h>

namespace facebook {
namespace eden {
/**
 * Helper function that optimizes for the case where a read-only check on a
 * contended data structure is likely to succeed. It first acquires the
 * synchronized object with an rlock. If check returns a true-ish value, then
 * the result of dereferencing it is returned. Otherwise, a wlock is acquired
 * and update is called.
 *
 * check should have type (const State&) -> std::optional<T>
 * update should have type (LockedPtr&) -> T
 */
template <typename Return, typename State, typename CheckFn, typename UpdateFn>
Return tryRlockCheckBeforeUpdate(
    folly::Synchronized<State>& state,
    CheckFn&& check,
    UpdateFn&& update) {
  // First, acquire the rlock. If the check succeeds, acquiring a wlock is
  // unnecessary.
  {
    auto rlock = state.rlock();
    auto result = check(*rlock);
    if (LIKELY(bool(result))) {
      return *std::move(result);
    }
  }

  auto wlock = state.wlock();
  // Check again - something may have raced between the locks.
  auto result = check(*wlock);
  if (UNLIKELY(bool(result))) {
    return *std::move(result);
  }

  return update(wlock);
}

} // namespace eden
} // namespace facebook
