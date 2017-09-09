/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/fuse/MountPoint.h"

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/ThreadName.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/async/EventBase.h>

#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <unistd.h>

#include "eden/fs/fuse/Dispatcher.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"

using namespace folly;

DEFINE_int32(fuseNumThreads, 16, "how many fuse dispatcher threads to spawn");

namespace facebook {
namespace eden {
namespace fusell {

MountPoint::MountPoint(AbsolutePathPiece path, Dispatcher* dispatcher)
    : path_(path), uid_(getuid()), gid_(getgid()), dispatcher_{dispatcher} {}

MountPoint::~MountPoint() {}

void MountPoint::start(
    folly::EventBase* eventBase,
    std::function<void()> onStop,
    bool debug) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (status_ != Status::UNINIT) {
    throw std::runtime_error("mount point has already been started");
  }

  eventBase_ = eventBase;
  onStop_ = onStop;

  status_ = Status::STARTING;

  auto fuseDevice = privilegedFuseMount(path_.stringPiece());
  channel_ =
      std::make_unique<FuseChannel>(std::move(fuseDevice), debug, dispatcher_);

  // Now, while holding the initialization mutex, start up the workers.
  threads_.reserve(FLAGS_fuseNumThreads);
  for (auto i = 0; i < FLAGS_fuseNumThreads; ++i) {
    threads_.emplace_back(std::thread([this] { fuseWorkerThread(); }));
  }

  // Wait until the mount is started successfully.
  while (status_ == Status::STARTING) {
    statusCV_.wait(lock);
  }
  if (status_ == Status::ERROR) {
    throw std::runtime_error("fuse session failed to initialize");
  }
}

void MountPoint::mountStarted() {
  std::lock_guard<std::mutex> guard(mutex_);
  // Don't update status_ if it has already been put into an error
  // state or something.
  if (status_ == Status::STARTING) {
    status_ = Status::RUNNING;
    statusCV_.notify_one();
  }
}

FuseChannel* MountPoint::getFuseChannel() const {
  return channel_.get();
}

void MountPoint::fuseWorkerThread() {
  setThreadName(to<std::string>("fuse", path_.basename()));

  // The channel is responsible for running the loop.  It will
  // continue to do so until the fuse session is exited, either
  // due to error or because the filesystem was unmounted, or
  // because FuseChannel::requestSessionExit() was called.
  channel_->processSession();

  bool shouldCallonStop = false;
  bool shouldJoin = false;

  {
    std::lock_guard<std::mutex> guard(mutex_);
    if (status_ == Status::STARTING) {
      // If we didn't get as far as setting the state to RUNNING,
      // we must have experienced an error
      status_ = Status::ERROR;
      statusCV_.notify_one();
      shouldJoin = true;
    } else if (status_ == Status::RUNNING) {
      // We are the first one to stop, so we get to share the news.
      status_ = Status::STOPPING;
      shouldCallonStop = true;
      shouldJoin = true;
    }
  }

  if (shouldJoin) {
    // We are the first thread to exit the loop; we get to arrange
    // to join and notify the server of our completion
    eventBase_->runInEventBaseThread([this, shouldCallonStop] {
      // Wait for all workers to be done
      for (auto& thr : threads_) {
        thr.join();
      }

      // and tear down the fuse session.  For a graceful restart,
      // we will want to FuseChannel::stealFuseDevice() before
      // this point, or perhaps pass it through the onStop_
      // call.
      channel_.reset();

      // Do a little dance to steal ownership of the indirect
      // reference to the EdenMount that is held by the
      // onStop_ function; we can't leave it owned by MountPoint
      // because that reference will block the completion of
      // the shutdown future.
      std::function<void()> stopper;
      std::swap(stopper, onStop_);

      // And let the edenMount know that all is done
      if (shouldCallonStop) {
        stopper();
      }
    });
  }
}

struct stat MountPoint::initStatData() const {
  struct stat st;
  memset(&st, 0, sizeof(st));

  st.st_uid = uid_;
  st.st_gid = gid_;
  // We don't really use the block size for anything.
  // 4096 is fairly standard for many file systems.
  st.st_blksize = 4096;

  return st;
}
}
}
} // facebook::eden::fusell
