/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Synchronized.h>
#include <unistd.h>

#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/utils/BucketedLog.h"

namespace facebook {
namespace eden {

class ProcessNameCache;

/**
 * An inexpensive mechanism for counting accesses by pids. Intended for counting
 * FUSE and Thrift calls from external processes.
 *
 * The first time a thread calls recordAccess, that thread is then coupled to
 * this particular ProcessAccessLog, even if it calls recordAccess on another
 * ProcessAccessLog instance. Thus, use one ProcessAccessLog per pool of
 * threads.
 */
class ProcessAccessLog {
 public:
  enum class AccessType : unsigned int {
    FuseRead,
    FuseWrite,
    FuseOther,
    FuseBackingStoreImport,
    Last,
  };

  explicit ProcessAccessLog(std::shared_ptr<ProcessNameCache> processNameCache);
  ~ProcessAccessLog();

  /**
   * Records an access by a process ID. The first call to recordAccess by a
   * particular thread binds that thread to this access log. Future recordAccess
   * calls on that thread will accumulate within this access log.
   *
   * Process IDs passed to recordAccess are also inserted into the
   * ProcessNameCache.
   */
  void recordAccess(pid_t pid, AccessType type);

  /**
   * Returns the number of times each pid was passed to recordAccess() in
   * `lastNSeconds`.
   *
   * Note: ProcessAccessLog buckets by whole seconds, so this number should be
   * considered an approximation.
   */
  std::unordered_map<pid_t, AccessCounts> getAccessCounts(
      std::chrono::seconds lastNSeconds);

 private:
  struct PerBucketAccessCounts {
    size_t counts[static_cast<int>(AccessType::Last)];

    size_t& operator[](AccessType type) {
      int idx = static_cast<int>(type);
      CHECK_LT(idx, static_cast<int>(AccessType::Last));
      return counts[idx];
    }
  };

  // Data for one second.
  struct Bucket {
    void clear();
    void add(pid_t pid, bool& isNew, AccessType type);
    void merge(const Bucket& other);

    std::unordered_map<pid_t, PerBucketAccessCounts> accessCountsByPid;
  };

  // Keep up to ten seconds of data, but use a power of two so BucketedLog
  // generates smaller, faster code.
  static constexpr uint64_t kBucketCount = 16;
  using Buckets = BucketedLog<Bucket, kBucketCount>;

  struct State {
    Buckets buckets;
  };

  const std::shared_ptr<ProcessNameCache> processNameCache_;
  folly::Synchronized<State> state_;

  friend struct ThreadLocalBucket;
};

} // namespace eden
} // namespace facebook
