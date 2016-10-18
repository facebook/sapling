/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "MountPoint.h"

#include "Channel.h"
#include "DirInode.h"
#include "FileInode.h"
#include "InodeBase.h"
#include "InodeDispatcher.h"
#include "InodeNameManager.h"

#include <sys/stat.h>
#include <string>
#include <vector>

namespace facebook {
namespace eden {
namespace fusell {

MountPoint::MountPoint(AbsolutePathPiece path, std::shared_ptr<DirInode> root)
    : path_(path),
      uid_(getuid()),
      gid_(getgid()),
      dispatcher_{new InodeDispatcher(this, std::move(root))},
      nameManager_{new InodeNameManager()} {}

MountPoint::~MountPoint() {}

void MountPoint::setRootInode(std::shared_ptr<DirInode> inode) {
  dispatcher_->setRootInode(std::move(inode));
}

std::shared_ptr<InodeBase> MountPoint::getInodeBaseForPath(
    RelativePathPiece path) const {
  auto inodeDispatcher = getDispatcher();
  auto inodeBase = inodeDispatcher->getInode(FUSE_ROOT_ID);
  auto relativePath = RelativePathPiece{path};

  // Walk down to the path of interest.
  auto it = relativePath.paths().begin();
  while (it != relativePath.paths().end()) {
    // This will throw if there is no such entry.
    inodeBase =
        inodeDispatcher
            ->lookupInodeBase(inodeBase->getNodeId(), it.piece().basename())
            .get();
    ++it;
  }

  return inodeBase;
}

std::shared_ptr<FileInode> MountPoint::getFileInodeForPath(
    RelativePathPiece path) const {
  auto inodeBase = getInodeBaseForPath(path);
  auto fileInode = std::dynamic_pointer_cast<FileInode>(inodeBase);
  if (fileInode) {
    return fileInode;
  } else {
    folly::throwSystemErrorExplicit(EISDIR);
  }
}

std::shared_ptr<DirInode> MountPoint::getDirInodeForPath(
    RelativePathPiece path) const {
  auto inodeBase = getInodeBaseForPath(path);
  auto dirInode = std::dynamic_pointer_cast<DirInode>(inodeBase);
  if (dirInode) {
    return dirInode;
  } else {
    folly::throwSystemErrorExplicit(ENOTDIR);
  }
}

void MountPoint::start(bool debug) {
  std::function<void()> onStop;
  return start(debug, onStop);
}

void MountPoint::start(bool debug, const std::function<void()>& onStop) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (status_ != Status::UNINIT) {
    throw std::runtime_error("mount point has already been started");
  }

  status_ = Status::STARTING;
  auto runner = [this, debug, onStop]() {
    try {
      this->run(debug);
    } catch (const std::exception& ex) {
      std::lock_guard<std::mutex> guard(mutex_);
      if (status_ == Status::STARTING) {
        LOG(ERROR) << "error starting FUSE mount: " << folly::exceptionStr(ex);
        startError_ = std::current_exception();
        status_ = Status::ERROR;
        statusCV_.notify_one();
        return;
      } else {
        // We potentially could call onStop() with a pointer to the exception,
        // or nullptr when stopping normally.
        LOG(ERROR) << "unhandled error occurred while running FUSE mount: "
                   << folly::exceptionStr(ex);
      }
    }
    if (onStop) {
      onStop();
    }
  };
  auto t = std::thread(runner);
  // Detach from the thread after starting it.
  // The onStop() function will be called to allow the caller to perform
  // any clean up desired.  However, since it runs from inside the thread
  // it can't join the thread yet.
  t.detach();

  // Wait until the mount is started successfully.
  while (status_ == Status::STARTING) {
    statusCV_.wait(lock);
  }
  if (status_ == Status::ERROR) {
    std::rethrow_exception(startError_);
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

void MountPoint::run(bool debug) {
  // This next line is responsible for indirectly calling mount().
  channel_ = std::make_unique<Channel>(this);
  channel_->runSession(dispatcher_.get(), debug);
  channel_.reset();
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
