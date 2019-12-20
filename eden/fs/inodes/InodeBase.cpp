/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeBase.h"

#include <folly/Likely.h>
#include <folly/logging/xlog.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/ParentInodeInfo.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/Clock.h"

namespace facebook {
namespace eden {

InodeBase::InodeBase(EdenMount* mount)
    : ino_{kRootNodeId},
      initialMode_{S_IFDIR | 0755},
      mount_{mount},
      location_{
          LocationInfo{nullptr,
                       PathComponentPiece{"", detail::SkipPathSanityCheck()}}} {
  XLOG(DBG5) << "root inode " << this << " (" << ino_ << ") created for mount "
             << mount_->getPath();
  // The root inode always starts with an implicit reference from FUSE.
  incFuseRefcount();

  mount->getInodeMetadataTable()->populateIfNotSet(
      ino_, [&] { return mount->getInitialInodeMetadata(S_IFDIR | 0755); });
}

InodeBase::InodeBase(
    InodeNumber ino,
    mode_t initialMode,
    const std::optional<InodeTimestamps>& initialTimestamps,
    TreeInodePtr parent,
    PathComponentPiece name)
    : ino_{ino},
      initialMode_{initialMode},
      mount_{parent->mount_},
      location_{LocationInfo{std::move(parent), name}} {
  // Inode numbers generally shouldn't be 0.
  // Older versions of glibc have bugs handling files with an inode number of 0
  DCHECK(ino_.hasValue());
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") created: " << getLogPath();

  mount_->getInodeMetadataTable()->populateIfNotSet(ino_, [&] {
    auto metadata = mount_->getInitialInodeMetadata(initialMode);
    if (initialTimestamps) {
      metadata.timestamps = *initialTimestamps;
    }
    return metadata;
  });
}

InodeBase::~InodeBase() {
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") destroyed: " << getLogPath();
}

// See Dispatcher::getattr
folly::Future<Dispatcher::Attr> InodeBase::getattr() {
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> InodeBase::setxattr(
    folly::StringPiece /*name*/,
    folly::StringPiece /*value*/,
    int /*flags*/) {
  // setxattr is not supported for any type of inode. This instructs the kernel
  // to automatically fail all future setxattr() syscalls with EOPNOTSUPP.
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> InodeBase::removexattr(folly::StringPiece /*name*/) {
  // removexattr is not supported for any type of inode. This instructs the
  // kernel to automatically fail all future removexattr() syscalls with
  // EOPNOTSUPP.
  FUSELL_NOT_IMPL();
}

folly::Future<folly::Unit> InodeBase::access(int /*mask*/) {
  // Returning ENOSYS instructs FUSE that access() will always succeed, so does
  // not need to call back into the FUSE daemon.
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
    if (parent->ino_ == kRootNodeId) {
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

std::optional<RelativePath> InodeBase::getPath() const {
  if (ino_ == kRootNodeId) {
    return RelativePath();
  }

  std::vector<PathComponent> names;
  if (!getPathHelper(names, true)) {
    return std::nullopt;
  }
  return RelativePath(names);
}

std::string InodeBase::getLogPath() const {
  if (ino_ == kRootNodeId) {
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
  return std::move(path).value();
}

void InodeBase::markUnlinkedAfterLoad() {
  auto loc = location_.wlock();
  DCHECK(!loc->unlinked);
  loc->unlinked = true;
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
  // onPtrRefZero() is const since we treat incrementing and decrementing the
  // pointer refcount as a non-modifying operation.  (The refcount is updated
  // atomically so the operation is thread-safe.)
  //
  // However when the last reference goes to zero we destroy the inode object,
  // which is a modifying operation.  Cast ourself back to non-const in this
  // case.  We are guaranteed that no-one else has a reference to us anymore so
  // this is safe.
  //
  // We could perhaps just make incrementPtrRef() and decrementPtrRef()
  // non-const instead.  InodePtr objects always point to non-const InodeBase
  // objects; we do not currently ever use pointer-to-const InodePtrs.
  getMount()->getInodeMap()->onInodeUnreferenced(
      const_cast<InodeBase*>(this), getParentInfo());
}

ParentInodeInfo InodeBase::getParentInfo() const {
  using ParentContentsPtr = folly::Synchronized<TreeInodeState>::LockedPtr;

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
        XLOG(DBG9) << "getParentInfo() acquired parent lock after " << numTries
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

InodeMetadata InodeBase::getMetadataLocked() const {
  return getMount()->getInodeMetadataTable()->getOrThrow(getNodeId());
}

void InodeBase::updateAtime() {
  // TODO: Is it worth implementing relatime-like logic?
  auto now = getNow();
  getMount()->getInodeMetadataTable()->modifyOrThrow(
      getNodeId(), [&](auto& metadata) { metadata.timestamps.atime = now; });
}

InodeTimestamps InodeBase::updateMtimeAndCtime(timespec now) {
  return getMount()
      ->getInodeMetadataTable()
      ->modifyOrThrow(
          getNodeId(),
          [&](auto& record) {
            record.timestamps.ctime = now;
            record.timestamps.mtime = now;
          })
      .timestamps;
}

timespec InodeBase::getNow() const {
  return getClock().getRealtime();
}

const Clock& InodeBase::getClock() const {
  return getMount()->getClock();
}

// Helper function to update Journal used by FileInode and TreeInode.
void InodeBase::updateJournal() {
  auto path = getPath();
  if (path.has_value()) {
    getMount()->getJournal().recordChanged(std::move(path.value()));
  }
}
} // namespace eden
} // namespace facebook
