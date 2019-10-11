/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <memory>

namespace facebook {
namespace eden {

/// Returns the pointer stored in `ptr` when `cond` is true, and it will call
/// `init` to create a new pointer and store it in `ptr` when `ptr` does not
/// contain anything (i.e. a nullptr).
///
/// Returns nullptr when `cond` is false, and it will set `ptr` to nullptr
/// when `ptr` contains something.
///
/// This function ensures `init` will only be called once when it is needed.
///
/// NOTE: `init` will be called after `ptr`'s write lock is acquired by the
/// function. Therefore, DO NOT try to acquire the lock of `ptr` nor call
/// `lazyInitialize` with the same `ptr` inside `init` since it will cause
/// deadlock.
///
template <typename T, typename Func>
std::shared_ptr<T> lazyInitialize(
    bool cond,
    folly::Synchronized<std::shared_ptr<T>>& ptr,
    Func&& init) {
  {
    auto rlock = ptr.rlock();

    if (cond && *rlock) {
      return *rlock;
    }

    if (!cond && !*rlock) {
      return nullptr;
    }
  }

  {
    auto wlock = ptr.wlock();

    if (cond) {
      *wlock = init();
    } else {
      *wlock = nullptr;
    }

    return *wlock;
  }
}

} // namespace eden
} // namespace facebook
