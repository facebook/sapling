/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodeMap.h"

#include <folly/Exception.h>
#include <folly/Likely.h>
#include <folly/experimental/logging/xlog.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/ParentInodeInfo.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/Bug.h"

using folly::Future;
using folly::Promise;
using folly::throwSystemErrorExplicit;
using folly::Unit;
using std::string;

namespace facebook {
namespace eden {

InodeMap::InodeMap(EdenMount* mount) : mount_{mount} {}

InodeMap::~InodeMap() {
  // TODO: We need to clean up the EdenMount / InodeMap destruction process a
  // bit.
  //
  // When an EdenMount is unmounted it should signal us that we are about to be
  // destroyed.  At that point we should:
  // - set a flag to immediately fail all future lookupInode() calls
  // - fail all pending lookup promises
  // - set a flag (maybe the same one as above) that causes us to immediately
  //   destroy inodes when their reference count drops to 0
  // - immediately destroy all loaded inodes whose reference count is already 0
  // - decrement our reference count on the root inode
  //
  // Only when the root inode count drops to 0 is it then safe to actually
  // destroy the EdenMount.
}

void InodeMap::initialize(TreeInodePtr root, fuse_ino_t maxExistingInode) {
  auto data = data_.wlock();
  CHECK(!root_);
  root_ = std::move(root);
  auto ret = data->loadedInodes_.emplace(FUSE_ROOT_ID, root_.get());
  CHECK(ret.second);
  DCHECK_GE(maxExistingInode, FUSE_ROOT_ID);
  data->nextInodeNumber_ = maxExistingInode + 1;
}

Future<InodePtr> InodeMap::lookupInode(fuse_ino_t number) {
  // Lock the data.
  // We hold it while doing most of our work below, but explicitly unlock it
  // before triggering inode loading or before fulfilling any Promises.
  auto data = data_.wlock();

  // Check to see if this Inode is already loaded
  auto loadedIter = data->loadedInodes_.find(number);
  if (loadedIter != data->loadedInodes_.end()) {
    // Make a copy of the InodePtr with the lock held, then release the lock
    // before calling makeFuture().
    //
    // This code path should be quite common, so it's better to perform
    // makeFuture()'s memory allocation without the lock held.
    auto result = InodePtr::newPtrLocked(loadedIter->second);
    data.unlock();
    return folly::makeFuture<InodePtr>(std::move(result));
  }

  // Look up the data in the unloadedInodes_ map.
  auto unloadedIter = data->unloadedInodes_.find(number);
  if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
    // This generally shouldn't happen.  If a fuse_ino_t has been allocated
    // we should always know about it.  It's a bug if our caller calls us with
    // an invalid fuse_ino_t number.
    auto bug = EDEN_BUG() << "InodeMap called with unknown inode number "
                          << number;
    return folly::makeFuture<InodePtr>(bug.toException());
  }

  // Check to see if anyone else has already started loading this inode.
  auto* unloadedData = &unloadedIter->second;
  bool alreadyLoading = !unloadedData->promises.empty();

  // Add a new entry to the promises list.
  unloadedData->promises.emplace_back();
  auto result = unloadedData->promises.back().getFuture();

  // If someone else has already started loading this inode we are done.
  // The current loading attempt will signal our promise when it completes.
  if (alreadyLoading) {
    return result;
  }

  // Walk up through the parents until we find a loaded TreeInode.
  // Once we find one, we break out, release the lock, and then call
  // loadChildInode() on it.  When the loadChildInode() future finishes
  // we have it signal all pending promises for that inode.
  //
  // For parents we don't find, add a promise that will trigger the lookup on
  // its necessary child.
  //
  // (It might have been simpler to recursively call lookupInode() to get the
  // parent, but that would require releasing and re-acquiring the lock more
  // than necessary.)
  auto childInodeNumber = number;
  while (true) {
    // Check to see if this parent is loaded
    loadedIter = data->loadedInodes_.find(unloadedData->parent);
    if (loadedIter != data->loadedInodes_.end()) {
      // We found a loaded parent.
      // Grab copies of the arguments we need for startChildLookup(),
      // with the lock still held.
      InodePtr firstLoadedParent = InodePtr::newPtrLocked(loadedIter->second);
      PathComponent requiredChildName = unloadedData->name;
      bool isUnlinked = unloadedData->isUnlinked;
      // Unlock the data before starting the child lookup
      data.unlock();
      // Trigger the lookup, then return to our caller.
      startChildLookup(
          firstLoadedParent, requiredChildName, isUnlinked, childInodeNumber);
      return result;
    }

    // Look up the parent in unloadedInodes_
    unloadedIter = data->unloadedInodes_.find(unloadedData->parent);
    if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
      // This shouldn't happen.  We must know about the parent inode number if
      // we knew about the child.
      auto bug = EDEN_BUG() << "unknown parent inode " << unloadedData->parent
                            << " (of " << unloadedData->name << ")";
      // Unlock our data before calling inodeLoadFailed()
      data.unlock();
      inodeLoadFailed(childInodeNumber, bug.toException());
      return result;
    }

    auto* parentData = &unloadedIter->second;
    alreadyLoading = !parentData->promises.empty();

    // Add a new entry to the promises list.
    // It should kick off loading of the current child inode when
    // it is fulfilled.
    parentData->promises.emplace_back();
    setupParentLookupPromise(
        parentData->promises.back(),
        unloadedData->name,
        unloadedData->isUnlinked,
        childInodeNumber);

    if (alreadyLoading) {
      // This parent is already being loaded.
      // We don't need to trigger any new loads ourself.
      return result;
    }

    // Continue around the loop to look up our parent's parent
    childInodeNumber = unloadedData->parent;
    unloadedData = parentData;
  }
}

void InodeMap::setupParentLookupPromise(
    Promise<InodePtr>& promise,
    PathComponentPiece childName,
    bool isUnlinked,
    fuse_ino_t childInodeNumber) {
  promise.getFuture()
      .then([
        name = PathComponent(childName),
        this,
        isUnlinked,
        childInodeNumber
      ](const InodePtr& inode) {
        startChildLookup(inode, name, isUnlinked, childInodeNumber);
      })
      .onError([this, childInodeNumber](const folly::exception_wrapper& ex) {
        // Fail all pending lookups on the child
        inodeLoadFailed(childInodeNumber, ex);
      });
}

void InodeMap::startChildLookup(
    const InodePtr& parent,
    PathComponentPiece childName,
    bool isUnlinked,
    fuse_ino_t childInodeNumber) {
  auto treeInode = parent.asTreePtrOrNull();
  if (!treeInode) {
    auto bug = EDEN_BUG() << "parent inode " << parent->getNodeId() << " of ("
                          << childName << ", " << childInodeNumber
                          << ") does not refer to a tree";
    auto ew = bug.toException();
    auto promises = extractPendingPromises(childInodeNumber);
    for (auto& promise : promises) {
      promise.setException(ew);
    }
    return;
  }

  if (isUnlinked) {
    // FIXME: The UnloadedData object needs to have enough data to recreate
    // this object, or we need to be able to load the info from the overlay.
    XLOG(FATAL) << "reloading unlinked inodes not implemented yet";
  }

  // Ask the TreeInode to load this child inode.
  //
  // (Inode lookups can also be triggered by TreeInode::getOrLoadChild().
  // In that case getOrLoadChild() will call shouldLoadChild() to tell if it
  // should start the load itself, or if the load is already in progress.)
  treeInode->loadChildInode(childName, childInodeNumber);
}

InodeMap::PromiseVector InodeMap::inodeLoadComplete(InodeBase* inode) {
  auto number = inode->getNodeId();
  XLOG(DBG5) << "successfully loaded inode " << number << ": "
             << inode->getLogPath();

  PromiseVector promises;
  try {
    auto data = data_.wlock();
    auto it = data->unloadedInodes_.find(number);
    CHECK(it != data->unloadedInodes_.end())
        << "failed to find unloaded inode data when finishing load of inode "
        << number;
    swap(promises, it->second.promises);

    inode->setFuseRefcount(it->second.numFuseReferences);

    // Insert the entry into loadedInodes_, and remove it from unloadedInodes_
    data->loadedInodes_.emplace(number, inode);
    data->unloadedInodes_.erase(it);
    return promises;
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error marking inode " << number
              << " loaded: " << folly::exceptionStr(ex);
    auto ew = folly::exception_wrapper{std::current_exception(), ex};
    for (auto& promise : promises) {
      promise.setException(ew);
    }
    return PromiseVector{};
  }
}

void InodeMap::inodeLoadFailed(
    fuse_ino_t number,
    const folly::exception_wrapper& ex) {
  XLOG(ERR) << "failed to load inode " << number << ": "
            << folly::exceptionStr(ex);
  auto promises = extractPendingPromises(number);
  for (auto& promise : promises) {
    promise.setException(ex);
  }
}

InodeMap::PromiseVector InodeMap::extractPendingPromises(fuse_ino_t number) {
  PromiseVector promises;
  {
    auto data = data_.wlock();
    auto it = data->unloadedInodes_.find(number);
    CHECK(it != data->unloadedInodes_.end())
        << "failed to find unloaded inode data when finishing load of inode "
        << number;
    swap(promises, it->second.promises);
  }
  return promises;
}

Future<TreeInodePtr> InodeMap::lookupTreeInode(fuse_ino_t number) {
  return lookupInode(number).then(
      [](const InodePtr& inode) { return inode.asTreePtr(); });
}

Future<FileInodePtr> InodeMap::lookupFileInode(fuse_ino_t number) {
  return lookupInode(number).then(
      [](const InodePtr& inode) { return inode.asFilePtr(); });
}

InodePtr InodeMap::lookupLoadedInode(fuse_ino_t number) {
  auto data = data_.rlock();
  auto it = data->loadedInodes_.find(number);
  if (it == data->loadedInodes_.end()) {
    return nullptr;
  }
  return InodePtr::newPtrLocked(it->second);
}

TreeInodePtr InodeMap::lookupLoadedTree(fuse_ino_t number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  return inode.asTreePtr();
}

FileInodePtr InodeMap::lookupLoadedFile(fuse_ino_t number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  return inode.asFilePtr();
}

UnloadedInodeData InodeMap::lookupUnloadedInode(fuse_ino_t number) {
  auto data = data_.rlock();
  auto it = data->unloadedInodes_.find(number);
  if (it == data->unloadedInodes_.end()) {
    // This generally shouldn't happen.  If a fuse_ino_t has been allocated
    // we should always know about it.  It's a bug if our caller calls us with
    // an invalid fuse_ino_t number.
    XLOG(ERR) << "InodeMap called with unknown inode number " << number;
    throwSystemErrorExplicit(EINVAL, "unknown inode number ", number);
  }

  return UnloadedInodeData(it->second.parent, it->second.name);
}

folly::Optional<RelativePath> InodeMap::getPathForInode(
    fuse_ino_t inodeNumber) {
  auto data = data_.rlock();
  return getPathForInodeHelper(inodeNumber, data);
}

folly::Optional<RelativePath> InodeMap::getPathForInodeHelper(
    fuse_ino_t inodeNumber,
    const folly::Synchronized<Members>::ConstLockedPtr& data) {
  auto loadedIt = data->loadedInodes_.find(inodeNumber);
  if (loadedIt != data->loadedInodes_.cend()) {
    // If the inode is loaded, return its RelativePath
    return loadedIt->second->getPath();
  } else {
    auto unloadedIt = data->unloadedInodes_.find(inodeNumber);
    if (unloadedIt != data->unloadedInodes_.cend()) {
      if (unloadedIt->second.isUnlinked) {
        return folly::none;
      }
      // If the inode is not loaded, return its parent's path as long as it's
      // parent isn't the root
      auto parent = unloadedIt->second.parent;
      if (parent == FUSE_ROOT_ID) {
        // The parent is the Eden mount root, just return its name (base case)
        return RelativePath(unloadedIt->second.name);
      }
      auto dir = getPathForInodeHelper(parent, data);
      if (!dir) {
        EDEN_BUG() << "unlinked parent inode " << parent
                   << "appears to contain non-unlinked child " << inodeNumber;
      }
      return *dir + unloadedIt->second.name;
    } else {
      throwSystemErrorExplicit(EINVAL, "unknown inode number ", inodeNumber);
    }
  }
}

void InodeMap::decFuseRefcount(fuse_ino_t number, uint32_t count) {
  auto data = data_.wlock();

  // First check in the loaded inode map
  auto loadedIter = data->loadedInodes_.find(number);
  if (loadedIter != data->loadedInodes_.end()) {
    // Acquire an InodePtr, so that we are always holding a pointer reference
    // on the inode when we decrement the fuse refcount.
    //
    // This ensures that onInodeUnreferenced() will be processed at some point
    // after decrementing the FUSE refcount to 0, even if there were no
    // outstanding pointer references before this.
    auto inode = InodePtr::newPtrLocked(loadedIter->second);
    // Now release our lock before decrementing the inode's FUSE reference
    // count and immediately releasing our pointer reference.
    data.unlock();
    inode->decFuseRefcount(count);
    return;
  }

  // If it wasn't loaded, it should be in the unloaded map
  auto unloadedIter = data->unloadedInodes_.find(number);
  if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
    EDEN_BUG() << "InodeMap::decFuseRefcount() called on unknown inode number "
               << number;
  }

  // Decrement the reference count in the unloaded entry
  auto& unloadedEntry = unloadedIter->second;
  CHECK_GE(unloadedEntry.numFuseReferences, count);
  unloadedEntry.numFuseReferences -= count;
  if (unloadedEntry.numFuseReferences <= 0) {
    // We can completely forget about this unloaded inode now.
    XLOG(DBG5) << "forgetting unloaded inode " << number << ": "
               << unloadedEntry.parent << ":" << unloadedEntry.name;
    data->unloadedInodes_.erase(unloadedIter);
  }
}

void InodeMap::save() {
  // TODO
}

Future<Unit> InodeMap::shutdown() {
  // Record that we are in the process of shutting down.
  auto future = Future<Unit>::makeEmpty();
  {
    auto data = data_.wlock();
    CHECK(!data->shutdownPromise.hasValue())
        << "shutdown() invoked more than once on InodeMap for "
        << mount_->getPath();
    data->shutdownPromise.assign(Promise<Unit>{});
    future = data->shutdownPromise->getFuture();
  }

  // Walk from the root of the tree down, finding all unreferenced inodes,
  // and immediately destroy them.
  //
  // Hold the the mountpoint-wide rename lock in shared mode while doing the
  // walk.  We want to make sure that we walk *all* children.  While doing the
  // walk we want to make sure that an Inode that hasn't been processed yet
  // cannot be moved from the unprocessed part of the tree into a processed
  // part of the tree.
  {
    auto renameLock = mount_->acquireSharedRenameLock();
    root_->unloadChildrenNow();
  }

  // Also walk loadedInodes_ to immediately destroy all unreferenced unlinked
  // inodes.  (There may be unlinked inodes that have no outstanding pointer
  // references, but outstanding FUSE references.)
  //
  // We walk normal inodes via the root since it is easier to hold the parent
  // TreeInode's contents lock as we walk down from the root.  However, we
  // can't find unlinked inodes that way.  For unlinked inodes we don't need to
  // hold the parent's contents lock, so scanning loadedInodes_ for them is
  // straightforward.
  {
    // The simplest way to unload the inodes is to simply acquire InodePtrs
    // to them, then let the normal pointer release process be responsible for
    // unloading them.
    std::vector<InodePtr> inodesToUnload;
    auto data = data_.wlock();
    for (const auto& entry : data->loadedInodes_) {
      if (!entry.second->isPtrAcquireCountZero()) {
        continue;
      }
      if (!entry.second->isUnlinked()) {
        continue;
      }
      inodesToUnload.push_back(InodePtr::newPtrLocked(entry.second));
    }
    // Release the lock, then release all of our InodePtrs to unload
    // the inodes.
    data.unlock();
    inodesToUnload.clear();
  }

  // Decrement our reference count on root_.
  // This method lets us manually drop our reference count while still
  // retaining our pointer.  When onInodeUnreferenced() is called for the root
  // we know that all inodes have been destroyed and we can complete shutdown.
  root_.manualDecRef();

  return future;
}

void InodeMap::shutdownComplete(
    folly::Synchronized<Members>::LockedPtr&& data) {
  // We manually dropped our reference count to the root inode in
  // beginShutdown().  Destroy it now, and call resetNoDecRef() on our pointer
  // to make sure it doesn't try to decrement the reference count again when
  // the pointer is destroyed.
  delete root_.get();
  root_.resetNoDecRef();

  // Unlock data_ before fulfilling the shutdown promise, just in case the
  // promise invokes a callback that calls some of our other methods that
  // may need to acquire this lock.
  auto* shutdownPromise = &data->shutdownPromise.value();
  data.unlock();
  shutdownPromise->setValue();
}

void InodeMap::onInodeUnreferenced(
    const InodeBase* inode,
    ParentInodeInfo&& parentInfo) {
  XLOG(DBG5) << "inode " << inode->getNodeId()
             << " unreferenced: " << inode->getLogPath();
  // Acquire our lock.
  auto data = data_.wlock();

  // Decrement the Inode's acquire count
  auto acquireCount = inode->decPtrAcquireCount();
  if (acquireCount != 1) {
    // Someone else has already re-acquired a reference to this inode.
    // We can't destroy it yet.
    return;
  }

  // Decide if we should unload the inode now, or wait until later.
  bool unloadNow = false;
  bool shuttingDown = data->shutdownPromise.hasValue();
  DCHECK(shuttingDown || inode != root_.get());
  if (shuttingDown) {
    // Check to see if this was the root inode that got unloaded.
    // This indicates that the shutdown is complete.
    if (inode == root_.get()) {
      shutdownComplete(std::move(data));
      return;
    }

    // Always unload Inode objects immediately when shutting down.
    // We can't destroy the EdenMount until all inodes get unloaded.
    unloadNow = true;
  } else if (parentInfo.isUnlinked() && inode->getFuseRefcount() == 0) {
    // This inode has been unlinked and has no outstanding FUSE references.
    // This inode can now be completely destroyed and forgotten about.
    unloadNow = true;
  } else {
    // In other cases:
    // - If the inode is materialized, we should never unload it.
    // - Otherwise, we have the option to unload it or not.
    //   For now we choose to always keep it loaded.
  }

  if (unloadNow) {
    unloadInode(
        inode,
        parentInfo.getParent().get(),
        parentInfo.getName(),
        parentInfo.isUnlinked(),
        data);
    if (!parentInfo.isUnlinked()) {
      const auto& parentContents = parentInfo.getParentContents();
      auto it = parentContents->entries.find(parentInfo.getName());
      CHECK(it != parentContents->entries.end());
      CHECK_EQ(it->second->getInode(), inode);
      it->second->clearInode();
    }
  }

  // If we unloaded the inode, only delete it after we release our locks.
  // Deleting it may cause its parent TreeInode to become unreferenced, causing
  // another recursive call to onInodeUnreferenced(), which will need to
  // reacquire the lock.
  data.unlock();
  parentInfo.reset();
  if (unloadNow) {
    delete inode;
  }
}

InodeMapLock InodeMap::lockForUnload() {
  return InodeMapLock{data_.wlock()};
}

void InodeMap::unloadInode(
    const InodeBase* inode,
    TreeInode* parent,
    PathComponentPiece name,
    bool isUnlinked,
    const InodeMapLock& lock) {
  return unloadInode(inode, parent, name, isUnlinked, lock.data_);
}

void InodeMap::unloadInode(
    const InodeBase* inode,
    TreeInode* parent,
    PathComponentPiece name,
    bool isUnlinked,
    const folly::Synchronized<Members>::LockedPtr& data) {
  // Update timestamps to overlay header on unloadInode
  inode->updateOverlayHeader();

  auto fuseCount = inode->getFuseRefcount();
  if (fuseCount > 0) {
    // Insert an unloaded entry
    XLOG(DBG5) << "unloading inode " << inode->getNodeId()
               << " with FUSE refcount=" << fuseCount << ": "
               << inode->getLogPath();
    auto unloadedEntry =
        UnloadedInode(inode->getNodeId(), parent->getNodeId(), name);
    unloadedEntry.numFuseReferences = fuseCount;
    unloadedEntry.isUnlinked = isUnlinked;
    auto reverseInsertKey =
        std::make_pair(unloadedEntry.parent, unloadedEntry.name.piece());
    XLOG(DBG7) << "reverse unload emplace " << reverseInsertKey.first << ":"
               << reverseInsertKey.second;
    auto ret = data->unloadedInodes_.emplace(
        inode->getNodeId(), std::move(unloadedEntry));
    CHECK(ret.second);
  } else {
    XLOG(DBG5) << "forgetting unreferenced inode " << inode->getNodeId() << ": "
               << inode->getLogPath();
    // If this inode is unlinked, it can never be loaded again, since there are
    // no outstanding pointer or fuse references to it, and it cannot be
    // accessed by path either.  Delete any data about it from the overlay in
    // this case.
    if (isUnlinked) {
      mount_->getOverlay()->removeOverlayData(inode->getNodeId());
    }
  }

  auto numErased = data->loadedInodes_.erase(inode->getNodeId());
  CHECK_EQ(numErased, 1) << "inconsistent loaded inodes data: "
                         << inode->getLogPath();
}

bool InodeMap::shouldLoadChild(
    TreeInode* parent,
    PathComponentPiece name,
    fuse_ino_t childInode,
    folly::Promise<InodePtr> promise) {
  auto data = data_.wlock();
  CHECK(childInode < data->nextInodeNumber_);
  auto iter = data->unloadedInodes_.find(childInode);
  UnloadedInode* unloadedData{nullptr};
  if (iter == data->unloadedInodes_.end()) {
    // This happens when the inode was stored on disk by a previous instance of
    // the eden daemon and this is the first time we have been asked to load it
    // into memory.
    //
    // Insert a new entry into data->unloadedInodes_
    fuse_ino_t parentNumber = parent->getNodeId();
    auto newUnloadedData = UnloadedInode(childInode, parentNumber, name);
    auto ret =
        data->unloadedInodes_.emplace(childInode, std::move(newUnloadedData));
    DCHECK(ret.second);
    unloadedData = &ret.first->second;
  } else {
    unloadedData = &iter->second;
    DCHECK_EQ(unloadedData->number, childInode);
  }

  bool isFirstPromise = unloadedData->promises.empty();

  // Add the promise to the existing list for this inode.
  unloadedData->promises.push_back(std::move(promise));

  // If this is the very first promise then tell the caller they need
  // to start the load operation.  Otherwise someone else (whoever added the
  // first promise) has already started loading the inode.
  return isFirstPromise;
}

fuse_ino_t InodeMap::newChildLoadStarted(
    TreeInode* parent,
    PathComponentPiece name,
    folly::Promise<InodePtr> promise) {
  auto data = data_.wlock();

  // Allocate a new inode number
  auto childNumber = allocateInodeNumber(*data);

  // Put an entry in unloadedInodes_
  fuse_ino_t parentNumber = parent->getNodeId();
  auto unloadedData = UnloadedInode(childNumber, parentNumber, name);
  unloadedData.promises.push_back(std::move(promise));
  data->unloadedInodes_.emplace(childNumber, std::move(unloadedData));

  // Return the inode number
  return childNumber;
}

fuse_ino_t InodeMap::allocateInodeNumber() {
  auto data = data_.wlock();
  return allocateInodeNumber(*data);
}

void InodeMap::inodeCreated(const InodePtr& inode) {
  XLOG(DBG4) << "created new inode " << inode->getNodeId() << ": "
             << inode->getLogPath();
  auto data = data_.wlock();
  data->loadedInodes_.emplace(inode->getNodeId(), inode.get());
}

fuse_ino_t InodeMap::allocateInodeNumber(Members& data) {
  // fuse_ino_t should generally be 64-bits wide, in which case it isn't even
  // worth bothering to handle the case where nextInodeNumber_ wraps.
  // We don't need to bother checking for conflicts with existing inode numbers
  // since this can only happen if we wrap around.
  static_assert(
      sizeof(data.nextInodeNumber_) >= 8,
      "expected fuse_ino_t to be at least 64-bits");
  fuse_ino_t number = data.nextInodeNumber_;
  ++data.nextInodeNumber_;
  return number;
}
}
}
