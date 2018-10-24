/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/ProcessNameCache.h"
#include <folly/MapUtil.h>
#include "eden/fs/utils/Synchronized.h"

using namespace std::literals;

namespace facebook::eden::detail {

ProcPidExe getProcPidExe(pid_t pid) {
  ProcPidExe path;
  memcpy(path.data(), "/proc/", 6);
  auto digits = folly::uint64ToBufferUnsafe(pid, path.data() + 6);
  memcpy(path.data() + 6 + digits, "/exe", 5);
  return path;
}

std::string readPidName(pid_t pid) {
  char target[256];
  ssize_t rv = readlink(getProcPidExe(pid).data(), target, sizeof(target));
  if (rv == -1) {
    return folly::to<std::string>("<err:", errno, ">");
  } else {
    // Could do something fancy if the entire buffer is filled, but it's better
    // if this code does as few syscalls as possible, so just truncate the
    // result.
    return std::string{target, target + rv};
  }
}
} // namespace facebook::eden::detail

namespace facebook {
namespace eden {

ProcessNameCache::ProcessNameCache(std::chrono::nanoseconds expiry)
    : expiry_{expiry}, startPoint_{std::chrono::steady_clock::now()} {}

void ProcessNameCache::add(pid_t pid) {
  // add() is called by very high-throughput, low-latency code, such as the
  // FUSE processing loop. To optimize for the common case where pid's name is
  // already known, this code aborts early when we can acquire a reader lock.
  //
  // Another possible implementation strategy is to look up executable names on
  // a single background thread, meaning add() would always be a single
  // insertion into an MPSC queue. getAllProcessNames() would send a promise to
  // the thread to be fulfilled.

  auto now = std::chrono::steady_clock::now() - startPoint_;

  tryRlockCheckBeforeUpdate<folly::Unit>(
      state_,
      [&](const auto& state) -> std::optional<folly::Unit> {
        auto entry = folly::get_ptr(state.names, pid);
        if (entry) {
          entry->lastAccess.store(now, std::memory_order_seq_cst);
          return folly::unit;
        }
        return std::nullopt;
      },
      [&](auto& wlock) -> folly::Unit {
        auto& state = *wlock;

        // TODO: Perhaps this readlink() should be put onto a background thread.
        // The upside of doing it here is that the process is guaranteed to
        // exist because it's waiting for a response from Eden. The downside is
        // that responding to the caller is blocked on Eden looking up the
        // caller's executable name.
        state.names.emplace(pid, ProcessName{detail::readPidName(pid), now});

        // Bump the water level by two so that it's guaranteed to catch up.
        // Imagine names.size() == 200 with waterLevel = 0, and add() is
        // called sequentially with new pids. We wouldn't ever catch up and
        // clear expired ones. Thus, waterLevel should grow faster than
        // names.size().
        state.waterLevel += 2;
        if (state.waterLevel > state.names.size()) {
          clearExpired(now, state);
          state.waterLevel = 0;
        }

        return folly::unit;
      });
}

std::map<pid_t, std::string> ProcessNameCache::getAllProcessNames() {
  auto now = std::chrono::steady_clock::now() - startPoint_;

  auto state = state_.wlock();

  clearExpired(now, *state);

  std::map<pid_t, std::string> result;
  for (const auto& entry : state->names) {
    result[entry.first] = entry.second.name;
  }

  return result;
}

void ProcessNameCache::clearExpired(
    std::chrono::steady_clock::duration now,
    State& state) {
  // TODO: When we can rely on C++17, it might be cheaper to move the node
  // handles into another map and deallocate them outside of the lock.
  auto iter = state.names.begin();
  while (iter != state.names.end()) {
    auto next = std::next(iter);
    if (now - iter->second.lastAccess.load(std::memory_order_seq_cst) >
        expiry_) {
      state.names.erase(iter);
    }
    iter = next;
  }
}

} // namespace eden
} // namespace facebook
