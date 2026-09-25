/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifdef __linux__

#include <cstdint>
#include <stdexcept>
#include <string>

namespace facebook::eden {

struct CgroupFileCacheReclaimOptions {
  /** Reclaim only what active_file plus inactive_file exceeds this by. */
  uint64_t targetBytes{0};
  /** Upper bound on the bytes requested in one pass. Zero means no bound. */
  uint64_t maxReclaimBytes{0};
};

struct CgroupFileCacheReclaimResult {
  uint64_t fileCacheBytesBefore{0};
  uint64_t fileCacheBytesAfter{0};
  uint64_t requestedBytes{0};

  bool operator==(const CgroupFileCacheReclaimResult&) const = default;
};

/** The cgroup this process runs in is not named like an EdenFS cgroup. */
class NotEdenFsCgroupError : public std::runtime_error {
  using std::runtime_error::runtime_error;
};

/** The running kernel does not offer the memory.reclaim interface needed. */
class UnsupportedKernelError : public std::runtime_error {
  using std::runtime_error::runtime_error;
};

/**
 * Asks the kernel to shrink the file cache of the cgroup v2 EdenFS runs in
 * by writing to its memory.reclaim. The write is synchronous and can take a
 * while for large requests, so callers should keep it off latency sensitive
 * threads.
 */
class CgroupFileCacheReclaimer {
 public:
  explicit CgroupFileCacheReclaimer(
      std::string procSelfCgroupPath = "/proc/self/cgroup",
      std::string procSelfMountInfoPath = "/proc/self/mountinfo");

  /**
   * Requests min(excess over target, maxReclaimBytes) bytes. Returns the
   * file cache size before and after along with the amount requested, which
   * is zero when the cache was already at or below the target.
   *
   * Only acts on a cgroup whose name starts with "edenfs", which is how
   * systemd cgroup isolation and systemd lifecycle management place the
   * daemon; anything else throws NotEdenFsCgroupError before any cgroup
   * file is read. Throws UnsupportedKernelError when memory.reclaim is
   * missing or rejects the request syntax, and a generic exception when the
   * cgroup cannot be found, is not a leaf domain cgroup containing this
   * process, or the write fails.
   */
  CgroupFileCacheReclaimResult reclaim(
      const CgroupFileCacheReclaimOptions& options) const;

 private:
  std::string procSelfCgroupPath_;
  std::string procSelfMountInfoPath_;
};

} // namespace facebook::eden

#endif // __linux__
