/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodeBase.h"

#include <folly/Likely.h>
#include <folly/experimental/logging/xlog.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ParentInodeInfo.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/Clock.h"

using namespace folly;

namespace facebook {
namespace eden {

InodeBase::~InodeBase() {
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") destroyed: " << getLogPath();
}

InodeBase::InodeBase(EdenMount* mount)
    : ino_{FUSE_ROOT_ID},
      type_{dtype_t::Dir},
      mount_{mount},
      location_{
          LocationInfo{nullptr,
                       PathComponentPiece{"", detail::SkipPathSanityCheck()}}} {
  XLOG(DBG5) << "root inode " << this << " (" << ino_ << ") created for mount "
             << mount_->getPath();
  // The root inode always starts with an implicit reference from FUSE.
  incFuseRefcount();
}

InodeBase::InodeBase(
    fuse_ino_t ino,
    dtype_t type,
    TreeInodePtr parent,
    PathComponentPiece name)
    : ino_{ino},
      type_{type},
      mount_{parent->mount_},
      location_{LocationInfo{std::move(parent), name}} {
  // Inode numbers generally shouldn't be 0.
  // Older versions of glibc have bugs handling files with an inode number of 0
  DCHECK_NE(ino_, 0);
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") created: " << getLogPath();
}

// Helper function to set the timestamps of InodeTimestamps.
void InodeBase::InodeTimestamps::setTimestampValues(
    const struct timespec& timeStamp) {
  atime = timeStamp;
  ctime = timeStamp;
  mtime = timeStamp;
}

// See Dispatcher::getattr
folly::Future<fusell::Dispatcher::Attr> InodeBase::getattr() {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::setxattr(
    folly::StringPiece /*name*/,
    folly::StringPiece /*value*/,
    int /*flags*/) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::string> InodeBase::getxattr(folly::StringPiece /*name*/) {
  FUSELL_NOT_IMPL();
}
folly::Future<std::vector<std::string>> InodeBase::listxattr() {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::removexattr(folly::StringPiece /*name*/) {
  FUSELL_NOT_IMPL();
}
folly::Future<folly::Unit> InodeBase::access(int /*mask*/) {
  FUSELL_NOT_IMPL();
}

bool InodeBase::isUnlinked() const {
  auto loc = location_.rlock();
  return loc->unlinked;
}

/**
 * Helper function for getPath() and getLogPath()
 *
 * Populates the names vector with the list of PathComponents from the root
 * down to this inode.
 *
 * This method should not be called on the root inode.  The caller is
 * responsible for checking that before calling getPathHelper().
 *
 * Returns true if the the file exists at the given path, or false if the file
 * has been unlinked.
 *
 * If stopOnUnlinked is true, it breaks immediately when it finds that the file
 * has been unlinked.  The contents of the names vector are then undefined if
 * the function returns false.
 *
 * If stopOnUnlinked is false it continues building the names vector even if
 * the file is unlinked, which will then contain the path that the file used to
 * exist at.  (This path should be used only for logging purposes at that
 * point.)
 */
bool InodeBase::getPathHelper(
    std::vector<PathComponent>& names,
    bool stopOnUnlinked) const {
  TreeInodePtr parent;
  bool unlinked = false;
  {
    auto loc = location_.rlock();
    if (loc->unlinked) {
      if (stopOnUnlinked) {
        return false;
      }
      unlinked = true;
    }
    parent = loc->parent;
    // Our caller should ensure that we are not the root
    DCHECK(parent);
    names.push_back(loc->name);
  }

  while (true) {
    // Stop at the root inode.
    // We check for this based on inode number so we can stop without having to
    // acquire the root inode's location lock.  (Otherwise all path lookups
    // would have to acquire the root's lock, making it more likely to be
    // contended.)
    if (parent->ino_ == FUSE_ROOT_ID) {
      // Reverse the names vector, since we built it from bottom to top.
      std::reverse(names.begin(), names.end());
      return !unlinked;
    }

    auto loc = parent->location_.rlock();
    // In general our parent should not be unlinked if we are not unlinked,
    // which we checked above.  However, we have since released our location
    // lock, so it's possible (but unlikely) that someone unlinked us and our
    // parent directories since we checked above.
    if (UNLIKELY(loc->unlinked)) {
      if (stopOnUnlinked) {
        return false;
      }
      unlinked = true;
    }
    names.push_back(loc->name);
    parent = loc->parent;
    DCHECK(parent);
  }
}

folly::Optional<RelativePath> InodeBase::getPath() const {
  if (ino_ == FUSE_ROOT_ID) {
    return RelativePath();
  }

  std::vector<PathComponent> names;
  if (!getPathHelper(names, true)) {
    return folly::none;
  }
  return RelativePath(names);
}

std::string InodeBase::getLogPath() const {
  if (ino_ == FUSE_ROOT_ID) {
    // We use "<root>" here instead of the empty string to make log messages
    // more understandable.  The empty string would likely be confusing, as it
    // would appear if the file name were missing.
    return "<root>";
  }

  std::vector<PathComponent> names;
  bool unlinked = !getPathHelper(names, false);
  auto path = RelativePath(names);
  if (unlinked) {
    return folly::to<std::string>("<deleted:", path.stringPiece(), ">");
  }
  // TODO: We should probably adjust the PathFuncs code to use std::string
  // instead of fbstring.  For FB builds, std::string is the fbstring
  // implementation.  For external builds, with gcc 5+, std::string is very
  // similar to fbstring anyway.
  //
  // return std::move(path).value();
  return path.stringPiece().str();
}

std::unique_ptr<InodeBase> InodeBase::markUnlinked(
    TreeInode* parent,
    PathComponentPiece name,
    const RenameLock& renameLock) {
  XLOG(DBG5) << "inode " << this << " unlinked: " << getLogPath();
  DCHECK(renameLock.isHeld(mount_));

  {
    auto loc = location_.wlock();
    DCHECK(!loc->unlinked);
    DCHECK_EQ(loc->parent.get(), parent);
    loc->unlinked = true;
  }

  // Grab the inode map lock, and check if we should unload
  // ourself immediately.
  auto* inodeMap = getMount()->getInodeMap();
  auto inodeMapLock = inodeMap->lockForUnload();
  if (isPtrAcquireCountZero() && getFuseRefcount() == 0) {
    inodeMap->unloadInode(this, parent, name, true, inodeMapLock);
    // We have to delete ourself now.
    // Do this by returning a unique_ptr to ourself, so that our caller will
    // destroy us.  This ensures we get destroyed after releasing the InodeMap
    // lock.  Our calling TreeInode should wait to destroy us until they
    // release their contents lock as well.
    //
    // (Technically it should probably be fine even if the caller deletes us
    // before releasing their contents lock, it just seems safer to wait.
    // The main area of concern is that deleting us will drop a reference count
    // on our parent, which could require the code to acquire locks to destroy
    // our parent.  However, we are only ever invoked from unlink(), rmdir(),
    // or rename() operations which must already be holding a reference on our
    // parent.  Therefore our parent should never be destroyed when our
    // destructor gets invoked here, so we won't need to acquire our parent's
    // contents lock in our destructor.)
    return std::unique_ptr<InodeBase>(this);
  }
  // We don't need our caller to delete us, so return null.
  return nullptr;
}

void InodeBase::updateLocation(
    TreeInodePtr newParent,
    PathComponentPiece newName,
    const RenameLock& renameLock) {
  XLOG(DBG5) << "inode " << this << " renamed: " << getLogPath() << " --> "
             << newParent->getLogPath() << " / \"" << newName << "\"";
  DCHECK(renameLock.isHeld(mount_));
  DCHECK_EQ(mount_, newParent->mount_);

  auto loc = location_.wlock();
  DCHECK(!loc->unlinked);
  loc->parent = newParent;
  loc->name = newName.copy();
}

void InodeBase::onPtrRefZero() const {
  auto parentInfo = getParentInfo();
  getMount()->getInodeMap()->onInodeUnreferenced(this, std::move(parentInfo));
}

ParentInodeInfo InodeBase::getParentInfo() const {
  using ParentContentsPtr = folly::Synchronized<TreeInode::Dir>::LockedPtr;

  // Grab our parent's contents_ lock.
  //
  // We need a retry loop here in case we get renamed or unlinked while trying
  // to acquire our parent's lock.
  //
  // (We could acquire the mount point rename lock first, to ensure that we
  // can't be renamed during this process.  However it seems unlikely that we
  // would get renamed or unlinked, so retrying seems probably better than
  // holding a mountpoint-wide lock.)
  size_t numTries = {0};
  while (true) {
    ++numTries;
    TreeInodePtr parent;
    // Get our current parent.
    {
      auto loc = location_.rlock();
      parent = loc->parent;
      if (loc->unlinked) {
        XLOG(DBG6) << "getParentInfo(): unlinked inode detected after "
                   << numTries << " tries";
        return ParentInodeInfo{
            loc->name, loc->parent, loc->unlinked, ParentContentsPtr{}};
      }
    }

    if (!parent) {
      // We are the root inode.
      DCHECK_EQ(numTries, 1);
      return ParentInodeInfo{
          PathComponentPiece{"", detail::SkipPathSanityCheck()},
          nullptr,
          false,
          ParentContentsPtr{}};
    }
    // Now grab our parent's contents lock.
    auto parentContents = parent->getContents().wlock();

    // After acquiring our parent's contents lock we have to make sure it is
    // actually still our parent.  If it is we are done and can break out of
    // this loop.
    {
      auto loc = location_.rlock();
      if (loc->unlinked) {
        // This file was unlinked since we checked earlier
        XLOG(DBG6) << "getParentInfo(): file is newly unlinked on try "
                   << numTries;
        return ParentInodeInfo{
            loc->name, loc->parent, loc->unlinked, ParentContentsPtr{}};
      }
      if (loc->parent == parent) {
        // Our parent is still the same.  We're done.
        XLOG(DBG6) << "getParentInfo() acquired parent lock after " << numTries
                   << " tries";
        return ParentInodeInfo{
            loc->name, loc->parent, loc->unlinked, std::move(parentContents)};
      }
    }
    // Otherwise our parent changed, and we have to retry.
    parent.reset();
    parentContents.unlock();
  }
}

// See Dispatcher::setattr
folly::Future<fusell::Dispatcher::Attr> InodeBase::setattr(
    const struct stat& attr,
    int to_set) {
  // Check if gid and uid are same or not.
  if (to_set & (FUSE_SET_ATTR_UID | FUSE_SET_ATTR_GID)) {
    if ((to_set & FUSE_SET_ATTR_UID && attr.st_uid != getMount()->getUid()) ||
        (to_set & FUSE_SET_ATTR_GID && attr.st_gid != getMount()->getGid())) {
      folly::throwSystemErrorExplicit(
          EACCES, "changing the owner/group is not supported");
    }
    // Otherwise: there is no change
  }

  // Set FileInode or TreeInode specific data.
  return setInodeAttr(attr, to_set);
}

timespec InodeBase::getNow() const {
  return getMount()->getClock().getRealtime();
}

// Helper function to set timeStamps of FileInode and TreeInode
void InodeBase::setattrTimes(
    const struct stat& attr,
    int to_set,
    InodeTimestamps& timeStamps) {
  auto currentTime = getNow();

  // Set atime for TreeInode.
  if (to_set & FUSE_SET_ATTR_ATIME) {
    timeStamps.atime = attr.st_atim;
  } else if (to_set & FUSE_SET_ATTR_ATIME_NOW) {
    timeStamps.atime = currentTime;
  }

  // Set mtime for TreeInode.
  if (to_set & FUSE_SET_ATTR_MTIME) {
    timeStamps.mtime = attr.st_mtim;
  } else if (to_set & FUSE_SET_ATTR_MTIME_NOW) {
    timeStamps.mtime = currentTime;
  }

  // we do not allow users to set ctime using setattr. ctime should be changed
  // when ever setattr is called, as this function is called in setattr, update
  // ctime to currentTime.
  timeStamps.ctime = currentTime;
}

// Helper function to update Journal used by FileInode and TreeInode.
void InodeBase::updateJournal() {
  auto path = getPath();
  if (path.hasValue()) {
    getMount()->getJournal().addDelta(
        std::make_unique<JournalDelta>(JournalDelta{path.value()}));
  }
}
} // namespace eden
} // namespace facebook
