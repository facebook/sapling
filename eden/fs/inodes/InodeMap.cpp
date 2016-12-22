/*
 *  Copyright (c) 2016, Facebook, Inc.
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
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/utils/Bug.h"

using folly::Future;
using folly::Promise;
using folly::throwSystemErrorExplicit;
using std::string;

namespace facebook {
namespace eden {
InodeMap::InodeMap() {}

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

void InodeMap::setRootInode(TreeInodePtr root) {
  auto data = data_.wlock();
  CHECK(!root_);
  root_ = root;
  auto ret = data->loadedInodes_.emplace(FUSE_ROOT_ID, root);
  CHECK(ret.second);
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
    InodePtr result = loadedIter->second;
    data.unlock();
    return folly::makeFuture(result);
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
  auto* unloadedData = unloadedIter->second.get();
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
      InodePtr firstLoadedParent = loadedIter->second;
      PathComponent requiredChildName = unloadedData->name;
      // Unlock the data before starting the child lookup
      data.unlock();
      // Trigger the lookup, then return to our caller.
      startChildLookup(firstLoadedParent, requiredChildName, childInodeNumber);
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

    auto* parentData = unloadedIter->second.get();
    alreadyLoading = !parentData->promises.empty();

    // Add a new entry to the promises list.
    // It should kick off loading of the current child inode when
    // it is fulfilled.
    parentData->promises.emplace_back();
    setupParentLookupPromise(
        parentData->promises.back(), unloadedData->name, childInodeNumber);

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
    fuse_ino_t childInodeNumber) {
  promise.getFuture()
      .then([ name = PathComponent(childName), this, childInodeNumber ](
          const InodePtr& inode) {
        startChildLookup(inode, name, childInodeNumber);
      })
      .onError([this, childInodeNumber](const folly::exception_wrapper& ex) {
        // Fail all pending lookups on the child
        inodeLoadFailed(childInodeNumber, ex);
      });
}

void InodeMap::startChildLookup(
    const InodePtr& parent,
    PathComponentPiece childName,
    fuse_ino_t childInodeNumber) {
  auto treeInode = std::dynamic_pointer_cast<TreeInode>(parent);
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

  // Ask the TreeInode to load this child inode.
  //
  // (Inode lookups can also be triggered by TreeInode::getOrLoadChild().
  // In that case getOrLoadChild() will call shouldLoadChild() to tell if it
  // should start the load itself, or if the load is already in progress.)
  treeInode->loadChildInode(childName, childInodeNumber);
}

void InodeMap::inodeLoadComplete(const InodePtr& inode) {
  auto number = inode->getNodeId();
  VLOG(5) << "successfully loaded inode " << number << ": "
          << inode->getLogPath();

  PromiseVector promises;
  try {
    auto data = data_.wlock();
    auto it = data->unloadedInodes_.find(number);
    CHECK(it != data->unloadedInodes_.end())
        << "failed to find unloaded inode data when finishing load of inode "
        << number;
    swap(promises, it->second->promises);

    auto reverseKey = std::make_pair(
        it->second->parent, PathComponentPiece{it->second->name});
    auto reverseIter = data->unloadedInodesReverse_.find(reverseKey);
    CHECK(reverseIter != data->unloadedInodesReverse_.end())
        << "failed to find reverse unloaded inode data when finishing "
        << "load of inode " << number;

    // Insert the entry into loadedInodes_, and remove it from unloadedInodes_
    data->loadedInodes_.emplace(number, inode);
    data->unloadedInodesReverse_.erase(reverseIter);
    data->unloadedInodes_.erase(it);
  } catch (const std::exception& ex) {
    LOG(ERROR) << "error marking inode " << number
               << " loaded: " << folly::exceptionStr(ex);
    auto ew = folly::exception_wrapper{std::current_exception(), ex};
    for (auto& promise : promises) {
      promise.setException(ew);
    }
    return;
  }

  // Fulfill all of the pending promises after releasing our lock
  for (auto& promise : promises) {
    promise.setValue(inode);
  }
}

void InodeMap::inodeLoadFailed(
    fuse_ino_t number,
    const folly::exception_wrapper& ex) {
  LOG(ERROR) << "failed to load inode " << number << ": "
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
    swap(promises, it->second->promises);
  }
  return promises;
}

Future<TreeInodePtr> InodeMap::lookupTreeInode(fuse_ino_t number) {
  return lookupInode(number).then([](const InodePtr& inode) {
    auto tree = std::dynamic_pointer_cast<TreeInode>(inode);
    if (!tree) {
      throwSystemErrorExplicit(ENOTDIR);
    }
    return tree;
  });
}

Future<FileInodePtr> InodeMap::lookupFileInode(fuse_ino_t number) {
  return lookupInode(number).then([](const InodePtr& inode) {
    auto tree = std::dynamic_pointer_cast<FileInode>(inode);
    if (!tree) {
      throwSystemErrorExplicit(EISDIR);
    }
    return tree;
  });
}

InodePtr InodeMap::lookupLoadedInode(fuse_ino_t number) {
  auto data = data_.rlock();
  auto it = data->loadedInodes_.find(number);
  if (it == data->loadedInodes_.end()) {
    return nullptr;
  }
  return it->second;
}

TreeInodePtr InodeMap::lookupLoadedTree(fuse_ino_t number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  auto tree = std::dynamic_pointer_cast<TreeInode>(inode);
  if (!tree) {
    throwSystemErrorExplicit(ENOTDIR);
  }
  return tree;
}

FileInodePtr InodeMap::lookupLoadedFile(fuse_ino_t number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  auto file = std::dynamic_pointer_cast<FileInode>(inode);
  if (!file) {
    throwSystemErrorExplicit(EISDIR);
  }
  return file;
}

UnloadedInodeData InodeMap::lookupUnloadedInode(fuse_ino_t number) {
  auto data = data_.rlock();
  auto it = data->unloadedInodes_.find(number);
  if (it == data->unloadedInodes_.end()) {
    // This generally shouldn't happen.  If a fuse_ino_t has been allocated
    // we should always know about it.  It's a bug if our caller calls us with
    // an invalid fuse_ino_t number.
    LOG(ERROR) << "InodeMap called with unknown inode number " << number;
    throwSystemErrorExplicit(EINVAL, "unknown inode number ", number);
  }

  return UnloadedInodeData(it->second->parent, it->second->name);
}

void InodeMap::save() {
  // TODO
}

bool InodeMap::shouldLoadChild(
    TreeInode* parent,
    PathComponentPiece name,
    Promise<InodePtr> promise,
    fuse_ino_t* childInodeReturn) {
  auto data = data_.wlock();

  // Check in unloadedInodesReverse_ to see if we already have an inode
  // allocated for this entry.
  UnloadedInode* unloadedData{nullptr};
  auto reverseLookupKey = std::make_pair(parent->getNodeId(), name);
  auto reverseIter = data->unloadedInodesReverse_.find(reverseLookupKey);
  if (reverseIter == data->unloadedInodesReverse_.end()) {
    // No inode allocated for this entry yet.
    // Allocate one now.
    unloadedData = allocateUnloadedInode(*data, parent, name);
  } else {
    // We already have an inode assigned to this entry
    unloadedData = reverseIter->second;

    // Check to see if this inode is already in the process of being loaded.
    // If so, just add this promise to the existing list.  Return false to the
    // caller (TreeInode::getOrLoadChild()) indicating that it doesn't need to
    // start the load itself.
    if (!unloadedData->promises.empty()) {
      unloadedData->promises.push_back(std::move(promise));
      *childInodeReturn = unloadedData->number;
      return false;
    }
  }

  // Add the promise to the existing list for this inode.
  // Return true asking the caller to start the load now.
  unloadedData->promises.push_back(std::move(promise));
  *childInodeReturn = unloadedData->number;
  return true;
}

fuse_ino_t InodeMap::allocateInodeNumber() {
  auto data = data_.wlock();
  return allocateInodeNumber(*data);
}

void InodeMap::inodeCreated(const InodePtr& inode) {
  VLOG(4) << "created new inode " << inode->getNodeId() << ": "
          << inode->getLogPath();
  auto data = data_.wlock();
  data->loadedInodes_.emplace(inode->getNodeId(), inode);
}

fuse_ino_t InodeMap::getOrAllocateUnloadedInodeNumber(
    const TreeInode* parent,
    PathComponentPiece name) {
  auto data = data_.wlock();
  auto reverseKey = std::make_pair(parent->getNodeId(), name);
  auto reverseIter = data->unloadedInodesReverse_.find(reverseKey);
  if (reverseIter != data->unloadedInodesReverse_.end()) {
    // We already have an inode for this entry
    return reverseIter->second->number;
  }

  // We have to allocate an inode and insert it into the unloaded map
  auto* unloadedData = allocateUnloadedInode(*data, parent, name);
  return unloadedData->number;
}

InodeMap::UnloadedInode* InodeMap::allocateUnloadedInode(
    Members& data,
    const TreeInode* parent,
    PathComponentPiece name) {
  auto childNumber = allocateInodeNumber(data);

  // Put the child inode in unloadedInodes_
  fuse_ino_t parentNumber = parent->getNodeId();
  auto newUnloadedData =
      std::make_unique<UnloadedInode>(childNumber, parentNumber, name);
  auto* unloadedData = newUnloadedData.get();
  // When we insert the data in to unloadedInodesReverse_, make sure the
  // PathComponentPiece in the key points to the PathComponent owned by the
  // UnloadedInode, so that the memory is guaranteed to be valid for as long
  // as the entry exists.
  auto reverseInsertKey =
      std::make_pair(parentNumber, PathComponentPiece{newUnloadedData->name});
  data.unloadedInodes_.emplace(childNumber, std::move(newUnloadedData));
  data.unloadedInodesReverse_.emplace(reverseInsertKey, unloadedData);
  return unloadedData;
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
