/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/synchronization/LifoSem.h>
#include <vector>

#include "eden/common/utils/DirType.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/store/ObjectFetchContext.h"

namespace facebook::eden {

class StructuredLogger;
class EdenMount;

struct InodeAccess {
  InodeNumber inodeNumber;
  dtype_t dtype;
  ObjectFetchContext::Cause cause;
  std::optional<std::string> causeDetail;
  std::weak_ptr<EdenMount> edenMount;
};

class InodeAccessLogger {
 public:
  InodeAccessLogger(
      std::shared_ptr<ReloadableConfig> reloadableConfig,
      std::shared_ptr<StructuredLogger> structuredLogger);
  virtual ~InodeAccessLogger();

  /**
   * Puts a InodeAccess event on a worker thread to be processed asynchronously
   */
  virtual void logInodeAccess(InodeAccess access);

 private:
  struct State {
    bool workerThreadShouldStop = false;
    std::vector<InodeAccess> work;
  };

  /**
   * Returns true if the file access should not be logged based on if the
   * directory matches filtering rules
   */
  bool filterDirectory(folly::StringPiece directory, folly::StringPiece repo);

  /**
   * Uses the workerThread_ to process expensive computations for file
   * access events. Specifically, looking up the file path for an Inode
   */
  void processInodeAccessEvents();

  folly::Synchronized<State> state_;
  // We use a LifoSem here due to the fact that it is faster than a std::mutex
  // condition variable combination. It in general should be used in a case
  // in which performance is more important than fairness, and since this is
  // a single threaded worker, we don't care about fairness. See the header
  // file for this object for more information about its performance benefits.
  // Also, in general we use a semaphore here so the worker thread is not
  // spinning while the work queue is empty.
  folly::LifoSem sem_;
  std::thread workerThread_;

  std::shared_ptr<ReloadableConfig> reloadableConfig_;
  std::shared_ptr<StructuredLogger> structuredLogger_;
};

} // namespace facebook::eden
