/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <condition_variable>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>
#include "eden/fs/fuse/fuse_headers.h"
#include "eden/fs/utils/PathFuncs.h"

struct stat;

namespace folly {
class EventBase;
}

namespace facebook {
namespace eden {
namespace fusell {

class Dispatcher;
class FuseChannel;

class MountPoint {
 public:
  explicit MountPoint(AbsolutePathPiece path, Dispatcher* dispatcher);
  virtual ~MountPoint();

  const AbsolutePath& getPath() const {
    return path_;
  }

  /**
   * Mounts the filesystem in the VFS and spawns worker threads to
   * dispatch the fuse session.
   *
   * Returns as soon as the filesystem has been successfully mounted, or
   * as soon as the mount fails.
   *
   * The onStop argument will be called from the thread associated with
   * the provided eventBase after the mount point is stopped, but only
   * in the case that the mount was successfully initiated, and then
   * cleanly torn down.  In other words, if start() throws an exception,
   * onStop() will not be called.
   */
  void
  start(folly::EventBase* eventBase, std::function<void()> onStop, bool debug);

  uid_t getUid() const {
    return uid_;
  }

  gid_t getGid() const {
    return gid_;
  }

  /**
   * Indicate that the mount point has been successfully started.
   *
   * This function should only be invoked by the Dispatcher class.
   */
  void mountStarted();

  /**
   * Return a stat structure that has been minimally initialized with
   * data for this mount point.
   *
   * The caller must still initialize all file-specific data (inode number,
   * file mode, size, timestamps, link count, etc).
   */
  struct stat initStatData() const;

  /**
   * Returns the associated FuseChannel or nullptr if there is none assigned.
   */
  FuseChannel* getFuseChannel() const;

 private:
  enum class Status { UNINIT, STARTING, RUNNING, ERROR, STOPPING };

  // Dispatches fuse requests
  void fuseWorkerThread();

  // Forbidden copy constructor and assignment operator
  MountPoint(MountPoint const&) = delete;
  MountPoint& operator=(MountPoint const&) = delete;

  AbsolutePath const path_; // the path where this MountPoint is mounted
  uid_t uid_;
  gid_t gid_;

  Dispatcher* const dispatcher_;
  std::unique_ptr<FuseChannel> channel_;

  std::mutex mutex_;
  std::condition_variable statusCV_;
  Status status_{Status::UNINIT};

  std::vector<std::thread> threads_;
  folly::EventBase* eventBase_{nullptr};
  std::function<void()> onStop_;
};
}
}
} // facebook::eden::fusell
