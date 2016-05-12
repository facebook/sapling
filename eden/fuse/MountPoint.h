/*
 *  Copyright (c) 2016, Facebook, Inc.
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
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {
namespace fusell {

class DirInode;
class InodeDispatcher;
class InodeNameManager;
class Channel;

class MountPoint {
 public:
  explicit MountPoint(
      AbsolutePathPiece path,
      std::shared_ptr<DirInode> root = {});
  virtual ~MountPoint();

  void setRootInode(std::shared_ptr<DirInode> inode);

  const AbsolutePath& getPath() const {
    return path_;
  }

  InodeDispatcher* getDispatcher() const {
    return dispatcher_.get();
  }

  InodeNameManager* getNameMgr() const {
    return nameManager_.get();
  }

  /*
   * Spawn a new thread to mount the filesystem and run the fuse channel.
   *
   * This is similar to run(), except that it returns as soon as the filesystem
   * has been successfully mounted.
   *
   * If an onStop() argument is supplied, this will be called from the FUSE
   * channel thread after the mount point is stopped, just before the thread
   * terminates.  (This happens once the mount point is unmounted.)
   *
   * If start() throws an exception, onStop() will not be called.
   */
  void start(bool debug);
  void start(bool debug, const std::function<void()>& onStop);

  /*
   * Mount the file system, and run the fuse channel.
   *
   * This function will not return until the filesystem is unmounted.
   */
  void run(bool debug);

  uid_t getUid() const {
    return uid_;
  }

  gid_t getGid() const {
    return gid_;
  }

  /** Returns the channel associated with this mount point.
   * No smart pointer because the lifetime is managed solely
   * by the MountPoint instance.
   * This method may return nullptr during initialization or
   * finalization of a mount point.
   */
  Channel* getChannel() {
    return channel_.get();
  }

  /*
   * Indicate that the mount point has been successfully started.
   *
   * This function should only be invoked by InodeDispatcher.
   */
  void mountStarted();

 private:
  enum class Status { UNINIT, STARTING, RUNNING, ERROR };

  // Forbidden copy constructor and assignment operator
  MountPoint(MountPoint const&) = delete;
  MountPoint& operator=(MountPoint const&) = delete;

  AbsolutePath const path_; // the path where this MountPoint is mounted
  uid_t uid_;
  gid_t gid_;

  std::unique_ptr<InodeDispatcher> const dispatcher_;
  std::unique_ptr<InodeNameManager> const nameManager_;
  std::unique_ptr<Channel> channel_;

  std::mutex mutex_;
  std::condition_variable statusCV_;
  Status status_{Status::UNINIT};
  std::exception_ptr startError_;
};
}
}
} // facebook::eden::fusell
