/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/ProcessNameCache.h"
#include <folly/FileUtil.h>
#include <folly/MapUtil.h>
#include <folly/system/ThreadName.h>
#include <optional>
#include "eden/fs/utils/Synchronized.h"
#ifdef __APPLE__
#include <libproc.h> // @manual
#endif

using namespace std::literals;

namespace facebook::eden::detail {

ProcPidCmdLine getProcPidCmdLine(pid_t pid) {
  ProcPidCmdLine path;
  memcpy(path.data(), "/proc/", 6);
  auto digits = folly::uint64ToBufferUnsafe(pid, path.data() + 6);
  memcpy(path.data() + 6 + digits, "/cmdline", 9);
  return path;
}

std::string readPidName(pid_t pid) {
#ifdef __APPLE__
  char target[PROC_PIDPATHINFO_MAXSIZE];
  // libproc is undocumented and unsupported, but the implementation is open
  // source:
  // https://opensource.apple.com/source/xnu/xnu-2782.40.9/libsyscall/wrappers/libproc/libproc.c
  // The return value is 0 on error, otherwise is the length of the buffer.
  // It takes care of overflow/truncation.
  ssize_t rv = proc_pidpath(pid, target, sizeof(target));
  if (rv == 0) {
    return folly::to<std::string>("<err:", errno, ">");
  }
  return std::string{target, target + rv};
#else
  char target[256];
  const auto fd =
      folly::openNoInt(getProcPidCmdLine(pid).data(), O_RDONLY | O_CLOEXEC);
  if (fd == -1) {
    return folly::to<std::string>("<err:", errno, ">");
  }
  SCOPE_EXIT {
    folly::closeNoInt(fd);
  };

  ssize_t rv = folly::readFull(fd, target, sizeof(target));
  if (rv == -1) {
    return folly::to<std::string>("<err:", errno, ">");
  } else {
    // Could do something fancy if the entire buffer is filled, but it's better
    // if this code does as few syscalls as possible, so just truncate the
    // result.
    return std::string{target, target + rv};
  }
#endif
}
} // namespace facebook::eden::detail

namespace facebook {
namespace eden {

ProcessNameCache::ProcessNameCache(std::chrono::nanoseconds expiry)
    : expiry_{expiry}, startPoint_{std::chrono::steady_clock::now()} {
  workerThread_ = std::thread{[this] {
    folly::setThreadName("ProcessNameCacheWorker");
    processActions();
  }};
}

ProcessNameCache::~ProcessNameCache() {
  state_.wlock()->workerThreadShouldStop = true;
  sem_.post();
  workerThread_.join();
}

void ProcessNameCache::add(pid_t pid) {
  // add() is called by very high-throughput, low-latency code, such as the
  // FUSE processing loop. To optimize for the common case where pid's name is
  // already known, this code aborts early when we can acquire a reader lock.
  //
  // When the pid's name is not known, reading the pid's name is done on a
  // background thread for two reasons:
  //
  // 1. Making a syscall in this high-throughput, low-latency path would slow
  //  down the caller. Queuing work for a background worker is cheaper.
  //
  // 2. (At least on kernel (4.16.18) Reading from /proc/$pid/cmdline
  // acquires the mmap semaphore (mmap_sem) of the process in order to
  // safely probe the memory containing the command line. A page fault
  // also holds mmap_sem while it calls into the filesystem to read
  // the page. If the page is on a FUSE filesystem, the process will
  // call into FUSE while holding the mmap_sem. If the FUSE thread
  // tries to read from /proc/$pid/cmdline, it will wait for mmap_sem,
  // which won't be released because the owner is waiting for
  // FUSE. There's a small detail here that mmap_sem is a
  // reader-writer lock, so this scenario _usually_ works, since both
  // operations grab the lock for reading. However, if there is a
  // writer waiting on the lock, readers are forced to wait in order
  // to avoid starving the writer. (Thanks Omar Sandoval for the
  // analysis.)
  //
  // Thus, add() cannot ever block on the completion of reading
  // /proc/$pid/cmdline, which includes a blocking push to a bounded worker
  // queue and a read from the SharedMutex while a writer has it. The read from
  // /proc/$pid/cmdline must be done on a background thread while the state
  // lock is not held.
  //
  // The downside of placing the work on a background thread is that it's
  // possible for the process making a FUSE request to exit before its name
  // can be looked up.

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
        auto [iter, inserted] = wlock->addQueue.insert(pid);
        wlock.unlock();
        if (inserted) {
          sem_.post();
        }

        return folly::unit;
      });
}

std::map<pid_t, std::string> ProcessNameCache::getAllProcessNames() {
  auto [promise, future] =
      folly::makePromiseContract<std::map<pid_t, std::string>>();

  state_.wlock()->getQueue.emplace_back(std::move(promise));
  sem_.post();

  return std::move(future).get();
}

void ProcessNameCache::clearExpired(
    std::chrono::steady_clock::duration now,
    State& state) {
  // TODO: When we can rely on C++17, it might be cheaper to move the node
  // handles into another map and deallocate them outside of the lock.
  auto iter = state.names.begin();
  while (iter != state.names.end()) {
    auto next = std::next(iter);
    if (now - iter->second.lastAccess.load(std::memory_order_seq_cst) >=
        expiry_) {
      state.names.erase(iter);
    }
    iter = next;
  }
}

void ProcessNameCache::processActions() {
  // Double-buffered work queues.
  folly::F14FastSet<pid_t> addQueue;
  std::vector<folly::Promise<std::map<pid_t, std::string>>> getQueue;

  for (;;) {
    addQueue.clear();
    getQueue.clear();

    sem_.wait();

    {
      auto state = state_.wlock();
      if (state->workerThreadShouldStop) {
        // Shutdown is only initiated by the destructor and since gets
        // are blocking, this implies no gets can be pending.
        CHECK(state->getQueue.empty())
            << "ProcessNameCache destroyed while gets were pending!";
        return;
      }

      addQueue.swap(state->addQueue);
      getQueue.swap(state->getQueue);
    }

    // sem_.wait() consumed one count, but we know addQueue.size() +
    // getQueue.size() + (maybe done) were added. Since we will process all
    // entries at once, rather than waking repeatedly, consume the rest.
    if (addQueue.size() + getQueue.size()) {
      (void)sem_.tryWait(addQueue.size() + getQueue.size() - 1);
    }

    // Process all additions before any gets so none are missed. It does mean
    // add(1), get(), add(2), get() processed all at once would return both
    // 1 and 2 from both get() calls.
    //
    // TODO: It might be worth skipping this during ProcessNameCache shutdown,
    // even if it did mean any pending get() calls could miss pids added prior.
    //
    // As described in ProcessNameCache::add() above, it is critical this work
    // be done outside of the state lock.
    std::vector<std::pair<pid_t, std::string>> addedNames;
    for (auto pid : addQueue) {
      addedNames.emplace_back(pid, detail::readPidName(pid));
    }

    auto now = std::chrono::steady_clock::now() - startPoint_;

    // Now insert any new names into the synchronized data structure.
    if (!addedNames.empty()) {
      auto state = state_.wlock();
      for (auto& [pid, name] : addedNames) {
        state->names.emplace(pid, ProcessName{std::move(name), now});
      }

      // Bump the water level by two so that it's guaranteed to catch up.
      // Imagine names.size() == 200 with waterLevel = 0, and add() is
      // called sequentially with new pids. We wouldn't ever catch up and
      // clear expired ones. Thus, waterLevel should grow faster than
      // names.size().
      state->waterLevel += 2 * addedNames.size();
      if (state->waterLevel > state->names.size()) {
        clearExpired(now, *state);
        state->waterLevel = 0;
      }
    }

    if (!getQueue.empty()) {
      // TODO: There are a few possible optimizations here, but get() is so
      // rare that they're not worth worrying about.
      std::map<pid_t, std::string> allProcessNames;

      {
        auto state = state_.wlock();
        clearExpired(now, *state);
        for (const auto& [pid, name] : state->names) {
          allProcessNames[pid] = name.name;
        }
      }

      for (auto& promise : getQueue) {
        promise.setValue(allProcessNames);
      }
    }
  }
}

} // namespace eden
} // namespace facebook
