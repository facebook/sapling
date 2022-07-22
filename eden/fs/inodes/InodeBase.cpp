/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include "eden/fs/utils/NotImplemented.h"

namespace facebook::eden {

InodeBase::InodeBase(EdenMount* mount)
    : ino_{kRootNodeId},
      mount_{mount},
      initialMode_{S_IFDIR | 0755},
      // The root inode always starts with an implicit reference from FUSE.
      numFsReferences_{1},
      location_{LocationInfo{
          nullptr,
          PathComponentPiece{"", detail::SkipPathSanityCheck()}}} {
  XLOG(DBG5) << "root inode " << this << " (" << ino_ << ") created for mount "
             << mount_->getPath();

#ifndef _WIN32
  mount->getInodeMetadataTable()->populateIfNotSet(
      ino_, [&] { return mount->getInitialInodeMetadata(S_IFDIR | 0755); });
#endif
}

InodeBase::InodeBase(
    InodeNumber ino,
    mode_t initialMode,
    FOLLY_MAYBE_UNUSED const std::optional<InodeTimestamps>& initialTimestamps,
    TreeInodePtr parent,
    PathComponentPiece name)
    : ino_{ino},
      mount_{parent->mount_},
      initialMode_{initialMode},
      location_{LocationInfo{std::move(parent), name}} {
  // Inode numbers generally shouldn't be 0.
  // Older versions of glibc have bugs handling files with an inode number of 0
  XDCHECK(ino_.hasValue());
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") created: " << getLogPath();

#ifndef _WIN32
  mount_->getInodeMetadataTable()->populateIfNotSet(ino_, [&] {
    auto metadata = mount_->getInitialInodeMetadata(initialMode);
    if (initialTimestamps) {
      metadata.timestamps = *initialTimestamps;
    }
    return metadata;
  });
#endif
}

InodeBase::~InodeBase() {
  XLOG(DBG5) << "inode " << this << " (" << ino_
             << ") destroyed: " << getLogPath();
}

#ifndef _WIN32
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
#endif

bool InodeBase::isUnlinked() const {
  auto loc = location_.rlock();
  return loc->unlinked;
}

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
    XDCHECK(parent);
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
    XDCHECK(parent);
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

RelativePath InodeBase::getUnsafePath() const {
  if (ino_ == kRootNodeId) {
    return RelativePath();
  }

  std::vector<PathComponent> names;
  getPathHelper(names, false);
  return RelativePath{names};
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
  XDCHECK(!loc->unlinked);
  loc->unlinked = true;
}

std::unique_ptr<InodeBase> InodeBase::markUnlinked(
    TreeInode* parent,
    PathComponentPiece name,
    const RenameLock& renameLock) {
  XLOG(DBG5) << "inode " << this << " unlinked: " << getLogPath();
  XDCHECK(renameLock.isHeld(mount_));

  {
    auto loc = location_.wlock();
    XDCHECK(!loc->unlinked);
    XDCHECK_EQ(loc->parent.get(), parent);
    loc->unlinked = true;
  }

  // Grab the inode map lock, and check if we should unload
  // ourself immediately.
  auto* inodeMap = getMount()->getInodeMap();
  auto inodeMapLock = inodeMap->lockForUnload();
  if (isPtrAcquireCountZero() && getFsRefcount() == 0) {
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
  XDCHECK(renameLock.isHeld(mount_));
  XDCHECK_EQ(mount_, newParent->mount_);

  auto loc = location_.wlock();
  XDCHECK(!loc->unlinked);
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
      XDCHECK_EQ(numTries, 1u);
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

#ifndef _WIN32
InodeMetadata InodeBase::getMetadataLocked() const {
  return getMount()->getInodeMetadataTable()->getOrThrow(getNodeId());
}
#endif

void InodeBase::updateAtime() {
#ifndef _WIN32
  // TODO: Is it worth implementing relatime-like logic?
  auto now = getNow();
  getMount()->getInodeMetadataTable()->modifyOrThrow(
      getNodeId(), [&](auto& metadata) { metadata.timestamps.atime = now; });
#endif
}

void InodeBase::updateMtimeAndCtime(FOLLY_MAYBE_UNUSED EdenTimestamp now) {
#ifndef _WIN32
  XLOG(DBG9) << "Updating timestamps for : " << ino_;
  getMount()->getInodeMetadataTable()->modifyOrThrow(
      getNodeId(), [&](auto& record) {
        record.timestamps.ctime = now;
        record.timestamps.mtime = now;
      });
#endif
}

EdenTimestamp InodeBase::getNow() const {
  return EdenTimestamp{getClock().getRealtime()};
}

const Clock& InodeBase::getClock() const {
  return getMount()->getClock();
}

ObjectStore& InodeBase::getObjectStore() const {
  return *getMount()->getObjectStore();
}

// Helper function to update Journal used by FileInode and TreeInode.
void InodeBase::updateJournal() {
  auto path = getPath();
  if (path.has_value()) {
    getMount()->getJournal().recordChanged(std::move(path.value()));
  }
}
} // namespace facebook::eden
