/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/InodeMap.h"

#include <boost/polymorphic_cast.hpp>

#include <folly/Exception.h>
#include <folly/Likely.h>
#include <folly/chrono/Conv.h>
#include <folly/logging/xlog.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/ParentInodeInfo.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/SystemError.h"
#include "eden/fs/utils/TimeUtil.h"

using folly::Future;
using folly::Promise;
using folly::throwSystemErrorExplicit;
using folly::Unit;
using std::optional;

namespace facebook::eden {

InodeMap::UnloadedInode::UnloadedInode(
    InodeNumber parentNum,
    PathComponentPiece entryName)
    : parent(parentNum), name(entryName) {}

InodeMap::UnloadedInode::UnloadedInode(
    InodeNumber parentNum,
    PathComponentPiece entryName,
    bool isUnlinked,
    mode_t mode,
    std::optional<ObjectId> hash,
    uint32_t fsRefcount)
    : parent(parentNum),
      name(entryName),
      isUnlinked{isUnlinked},
      mode{mode},
      hash{hash},
      numFsReferences{fsRefcount} {
  if (folly::kIsWindows) {
    XDCHECK_LE(numFsReferences, 1u);
  }
}

InodeMap::UnloadedInode::UnloadedInode(
    TreeInode* parent,
    PathComponentPiece entryName,
    bool isUnlinked,
    std::optional<ObjectId> hash,
    uint32_t fsRefcount)
    : parent{parent->getNodeId()},
      name{entryName},
      isUnlinked{isUnlinked},
      // There is no asTree->getMode() we can call,
      // however, directories are always represented with
      // this specific mode bit pattern in eden so we can
      // force the value down here.
      mode{S_IFDIR | 0755},
      hash{hash},
      numFsReferences{fsRefcount} {
  if (folly::kIsWindows) {
    XDCHECK_LE(numFsReferences, 1u);
  }
}

InodeMap::UnloadedInode::UnloadedInode(
    FileInode* inode,
    TreeInode* parent,
    PathComponentPiece entryName,
    bool isUnlinked,
    uint32_t fsRefcount)
    : parent{parent->getNodeId()},
      name{entryName},
      isUnlinked{isUnlinked},
      mode{inode->getMode()},
      hash{inode->getBlobHash()},
      numFsReferences{fsRefcount} {
  if (folly::kIsWindows) {
    XDCHECK_LE(numFsReferences, 1u);
  }
}

InodeMap::InodeMap(
    EdenMount* mount,
    std::shared_ptr<ReloadableConfig> config,
    bool throwEstaleIfInodeIsMissing)
    : mount_{mount},
      config_{std::move(config)},
      throwEstaleIfInodeIsMissing_{throwEstaleIfInodeIsMissing} {}

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

inline void InodeMap::insertLoadedInode(
    const folly::Synchronized<Members>::LockedPtr& data,
    InodeBase* inode) {
  auto ret = data->loadedInodes_.emplace(inode->getNodeId(), inode);
  XCHECK(ret.second);
  if (inode->getType() == dtype_t::Dir) {
    ++data->numTreeInodes_;
  } else {
    ++data->numFileInodes_;
  }
}

void InodeMap::initializeRoot(
    const folly::Synchronized<Members>::LockedPtr& data,
    TreeInodePtr root) {
  XCHECK_EQ(data->loadedInodes_.size(), 0ul)
      << "cannot load InodeMap data over a populated instance";
  XCHECK_EQ(data->unloadedInodes_.size(), 0ul)
      << "cannot load InodeMap data over a populated instance";

  XCHECK(!root_);
  root_ = std::move(root);
  insertLoadedInode(data, root_.get());
  XDCHECK_EQ(1ul, data->numTreeInodes_);
  XDCHECK_EQ(0ul, data->numFileInodes_);
}

void InodeMap::initialize(TreeInodePtr root) {
  initializeRoot(data_.wlock(), std::move(root));
}

template <class... Args>
void InodeMap::initializeUnloadedInode(
    const folly::Synchronized<Members>::LockedPtr& data,
    InodeNumber parentIno,
    InodeNumber ino,
    Args&&... args) {
  auto unloadedEntry = UnloadedInode(parentIno, std::forward<Args>(args)...);
  auto result = data->unloadedInodes_.emplace(ino, std::move(unloadedEntry));
  if (!result.second) {
    auto message = fmt::format(
        "failed to emplace inode number {}; is it already present in the InodeMap?",
        ino);
    XLOG(ERR) << message;
    throw std::runtime_error(message);
  }
}

void InodeMap::initializeFromTakeover(
    TreeInodePtr root,
    const SerializedInodeMap& takeover) {
  auto data = data_.wlock();
  initializeRoot(data, std::move(root));

  for (const auto& entry : *takeover.unloadedInodes_ref()) {
    if (*entry.numFsReferences_ref() < 0) {
      auto message = folly::to<std::string>(
          "inode number ",
          *entry.inodeNumber_ref(),
          " has a negative numFsReferences number");
      XLOG(ERR) << message;
      throw std::runtime_error(message);
    }

    std::optional<ObjectId> hash;
    if (entry.hash_ref().has_value()) {
      const std::string& value = entry.hash_ref().value();
      if (value.empty()) {
        // LEGACY: Old versions of EdenFS sent the empty string to mean
        // materialized. When a BackingStore wants to support the empty ObjectId
        // as a valid identifier, remove this code path.
        hash = std::nullopt;
      } else {
        hash = ObjectId{value};
      }
    }
    initializeUnloadedInode(
        data,
        InodeNumber::fromThrift(*entry.parentInode_ref()),
        InodeNumber::fromThrift(*entry.inodeNumber_ref()),
        PathComponentPiece{*entry.name_ref()},
        *entry.isUnlinked_ref(),
        *entry.mode_ref(),
        std::move(hash),
        folly::to<uint32_t>(*entry.numFsReferences_ref()));
  }

  XLOG(DBG2) << "InodeMap initialized mount " << mount_->getPath()
             << " from takeover, " << data->unloadedInodes_.size()
             << " inodes registered";
}

void InodeMap::initializeFromOverlay(TreeInodePtr root, Overlay& overlay) {
  XCHECK(mount_->isWorkingCopyPersistent());

  XLOG(DBG2) << "Initializing InodeMap for " << mount_->getPath();

  auto data = data_.wlock();
  initializeRoot(data, std::move(root));

  std::vector<std::tuple<AbsolutePath, InodeNumber>> pending;
  pending.emplace_back(mount_->getPath(), root_->getNodeId());

  while (!pending.empty()) {
    auto [path, dirInode] = std::move(pending.back());
    pending.pop_back();

    auto dirEntries = overlay.loadOverlayDir(dirInode);
    for (const auto& [name, dirent] : dirEntries) {
      auto entryPath = path + name;
      auto ino = dirent.getInodeNumber();

      if (dirent.isDirectory()) {
        // Do a quick check in the overlay to check if EdenFS has an overlay
        // entry for this Inode. The assumption is that files on disk must
        // have a corresponding Overlay entry. Note that since Overlay
        // entries are also created for files that aren't on disk, this will
        // load inodes with no corresponding on-disk files.
        if (!overlay.hasOverlayData(ino)) {
          continue;
        }
        pending.emplace_back(std::move(entryPath), ino);
      }

      initializeUnloadedInode(
          data,
          dirInode,
          ino,
          name,
          false,
          dirent.getInitialMode(),
          dirent.getOptionalHash(),
          1);
    }
  }

  XLOG(DBG2) << "InodeMap initialized mount " << mount_->getPath()
             << " from overlay, " << data->unloadedInodes_.size()
             << " inodes registered";
}

ImmediateFuture<InodePtr> InodeMap::lookupInode(InodeNumber number) {
  // Lock the data.
  // We hold it while doing most of our work below, but explicitly unlock it
  // before triggering inode loading or before fulfilling any Promises.
  auto data = data_.wlock();

  // Check to see if this Inode is already loaded
  auto loadedIter = data->loadedInodes_.find(number);
  if (loadedIter != data->loadedInodes_.end()) {
    return loadedIter->second.getPtr();
  }

  // Look up the data in the unloadedInodes_ map.
  auto unloadedIter = data->unloadedInodes_.find(number);
  if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
    if (throwEstaleIfInodeIsMissing_) {
      XLOG(DBG3) << "NFS inode " << number << " stale";
      // windows does not have ESTALE. We need some other error to turn into the
      // nfs stale error. For now let's just let it throw.
#ifndef _WIN32
      return ImmediateFuture<InodePtr>{folly::Try<InodePtr>{
          std::system_error{std::error_code{ESTALE, std::system_category()}}}};
#endif
    }
    // This generally shouldn't happen.  If a InodeNumber has been allocated we
    // should always know about it.  It's a bug if our caller calls us with an
    // invalid InodeNumber number.
    return EDEN_BUG_FUTURE(InodePtr)
        << "InodeMap called with unknown inode number " << number;
  }

  // Check to see if anyone else has already started loading this inode.
  auto* unloadedData = &unloadedIter->second;
  bool alreadyLoading = !unloadedData->promises.empty();

  // Add a new entry to the promises list.
  unloadedData->promises.emplace_back();
  auto result = unloadedData->promises.back().getSemiFuture();

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
      InodePtr firstLoadedParent = loadedIter->second.getPtr();
      PathComponent requiredChildName = unloadedData->name;
      bool isUnlinked = unloadedData->isUnlinked;
      auto optionalHash = unloadedData->hash;
      auto mode = unloadedData->mode;
      // Unlock the data before starting the child lookup
      data.unlock();
      // Trigger the lookup, then return to our caller.
      startChildLookup(
          firstLoadedParent,
          requiredChildName,
          isUnlinked,
          childInodeNumber,
          optionalHash,
          mode);
      return result;
    }

    // Look up the parent in unloadedInodes_
    unloadedIter = data->unloadedInodes_.find(unloadedData->parent);
    if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
      // This shouldn't happen.  We must know about the parent inode number if
      // we knew about the child.
      auto bug = EDEN_BUG_EXCEPTION()
          << "unknown parent inode " << unloadedData->parent << " (of "
          << unloadedData->name << ")";
      // Unlock our data before calling inodeLoadFailed()
      data.unlock();
      inodeLoadFailed(childInodeNumber, bug);
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
        childInodeNumber,
        unloadedData->hash,
        unloadedData->mode);

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
    InodeNumber childInodeNumber,
    std::optional<ObjectId> hash,
    mode_t mode) {
  promise.getFuture()
      .thenValue([name = PathComponent(childName),
                  this,
                  isUnlinked,
                  childInodeNumber,
                  hash,
                  mode](const InodePtr& inode) {
        startChildLookup(inode, name, isUnlinked, childInodeNumber, hash, mode);
      })
      .thenError([this, childInodeNumber](const folly::exception_wrapper& ex) {
        // Fail all pending lookups on the child
        inodeLoadFailed(childInodeNumber, ex);
      });
}

void InodeMap::startChildLookup(
    const InodePtr& parent,
    PathComponentPiece childName,
    bool isUnlinked,
    InodeNumber childInodeNumber,
    std::optional<ObjectId> hash,
    mode_t mode) {
  auto treeInode = parent.asTreePtrOrNull();
  if (!treeInode) {
    auto bug = EDEN_BUG_EXCEPTION()
        << "parent inode " << parent->getNodeId() << " of (" << childName
        << ", " << childInodeNumber << ") does not refer to a tree";
    return inodeLoadFailed(childInodeNumber, bug);
  }

  if (isUnlinked) {
    treeInode->loadUnlinkedChildInode(childName, childInodeNumber, hash, mode);
    return;
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
  // Since XLOG only evaluates the arguments if it is going to log, its cheaper
  // in the most common case to not save the inode log path to a local variable
  XLOG(DBG5) << "successfully loaded inode " << number << ": "
             << inode->getLogPath();

  PromiseVector promises;
  try {
    auto data = data_.wlock();
    auto it = data->unloadedInodes_.find(number);
    XCHECK(it != data->unloadedInodes_.end())
        << "failed to find unloaded inode data when finishing load of inode "
        << number << ": " << inode->getLogPath();
    swap(promises, it->second.promises);

    inode->setChannelRefcount(it->second.numFsReferences);

    // Insert the entry into loadedInodes_, and remove it from unloadedInodes_
    insertLoadedInode(data, inode);
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
    InodeNumber number,
    const folly::exception_wrapper& ex) {
  XLOG(ERR) << "failed to load inode " << number << ": "
            << folly::exceptionStr(ex);
  auto promises = extractPendingPromises(number);
  for (auto& promise : promises) {
    promise.setException(ex);
  }
}

InodeMap::PromiseVector InodeMap::extractPendingPromises(InodeNumber number) {
  PromiseVector promises;
  {
    auto data = data_.wlock();
    auto it = data->unloadedInodes_.find(number);
    XCHECK(it != data->unloadedInodes_.end())
        << "failed to find unloaded inode data when finishing load of inode "
        << number;
    swap(promises, it->second.promises);
  }
  return promises;
}

ImmediateFuture<TreeInodePtr> InodeMap::lookupTreeInode(InodeNumber number) {
  return lookupInode(number).thenValue(
      [](const InodePtr& inode) { return inode.asTreePtr(); });
}

ImmediateFuture<FileInodePtr> InodeMap::lookupFileInode(InodeNumber number) {
  return lookupInode(number).thenValue(
      [](const InodePtr& inode) { return inode.asFilePtr(); });
}

InodePtr InodeMap::lookupLoadedInode(InodeNumber number) {
  auto data = data_.rlock();
  auto it = data->loadedInodes_.find(number);
  if (it == data->loadedInodes_.end()) {
    return nullptr;
  }
  return it->second.getPtr();
}

TreeInodePtr InodeMap::lookupLoadedTree(InodeNumber number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  return inode.asTreePtr();
}

FileInodePtr InodeMap::lookupLoadedFile(InodeNumber number) {
  auto inode = lookupLoadedInode(number);
  if (!inode) {
    return nullptr;
  }
  return inode.asFilePtr();
}

std::optional<RelativePath> InodeMap::getPathForInode(InodeNumber inodeNumber) {
  // TODO: Remove the rootInode from the loadedInodes_ when it is unreferenced.
  if (inodeNumber == kRootNodeId) {
    return RelativePath();
  }

  auto data = data_.rlock();
  return getPathForInodeHelper(inodeNumber, data);
}

std::optional<RelativePath> InodeMap::getPathForInodeHelper(
    InodeNumber inodeNumber,
    const folly::Synchronized<Members>::RLockedPtr& data) {
  auto loadedIt = data->loadedInodes_.find(inodeNumber);
  if (loadedIt != data->loadedInodes_.cend()) {
    // If the inode is loaded, return its RelativePath
    return loadedIt->second->getPath();
  } else {
    auto unloadedIt = data->unloadedInodes_.find(inodeNumber);
    if (unloadedIt != data->unloadedInodes_.cend()) {
      if (unloadedIt->second.isUnlinked) {
        return std::nullopt;
      }
      // If the inode is not loaded, return its parent's path as long as it's
      // parent isn't the root
      auto parent = unloadedIt->second.parent;
      if (parent == kRootNodeId) {
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
void InodeMap::decFsRefcount(InodeNumber number, uint32_t count) {
  InodePtr inodePtr;
  {
    auto data = data_.wlock();
    inodePtr = decFsRefcountHelper(data, number, count);
  }
  // Now release our lock before decrementing the inode's FS reference
  // count and immediately releasing our pointer reference.
  if (inodePtr) {
    inodePtr->decFsRefcount();
  }
}

InodePtr InodeMap::decFsRefcountHelper(
    folly::Synchronized<Members>::LockedPtr& data,
    InodeNumber number,
    uint32_t count,
    bool clearRefCount) {
  if (folly::kIsWindows) {
    XDCHECK_EQ(count, 1u);
  }

  // First check in the loaded inode map
  auto loadedIter = data->loadedInodes_.find(number);
  if (loadedIter != data->loadedInodes_.end()) {
    // Acquire an InodePtr, so that we are always holding a pointer reference
    // on the inode when we decrement the fs refcount.
    //
    // This ensures that onInodeUnreferenced() will be processed at some point
    // after decrementing the FS refcount to 0, even if there were no
    // outstanding pointer references before this.
    return loadedIter->second.getPtr();
  }

  // If it wasn't loaded, it should be in the unloaded map
  auto unloadedIter = data->unloadedInodes_.find(number);
  if (UNLIKELY(unloadedIter == data->unloadedInodes_.end())) {
    EDEN_BUG() << "InodeMap::decFsRefcount() called on unknown inode number "
               << number;
  }

  // Decrement the reference count in the unloaded entry
  auto& unloadedEntry = unloadedIter->second;
  if (clearRefCount) {
    unloadedEntry.numFsReferences = 0;
  } else {
    XCHECK_GE(unloadedEntry.numFsReferences, count);
    unloadedEntry.numFsReferences -= count;
  }

  if (unloadedEntry.numFsReferences == 0) {
    // We can completely forget about this unloaded inode now.
    XLOG(DBG5) << "forgetting unloaded inode " << number << ": "
               << unloadedEntry.parent << ":" << unloadedEntry.name;
    data->unloadedInodes_.erase(unloadedIter);
  }
  return nullptr;
}

void InodeMap::forgetStaleInodes() {
  // Note this functionality is pretty similar to the periodic Inode unloading
  // which unloads old inodes (EdenServer::unloadInodes). The key difference is
  // that this code unloads UNLINKED inodes where as that code unloads LINKED
  // inodes. That codepath only considers unloading inodes reachable from the
  // root inode which means the inodes must be linked to be considered for
  // unloading. We might want to merge the two, however at the time of writing
  // linked inode unloading is broken. We should consider merging
  // both linked and unlinked inode unloading in the future: T100682833
#ifndef _WIN32
  // We do not track metadata for inodes on Windows. This was originally because
  // ProjFS does not send most requests after a file is first read to EdenFS.
  // If we support NFS on Windows, we need to keep track of inode metadata
  // on Windows to enable periodic inode unloading: T100720928.

  // TODO add a checkout decRef counter
  // TODO: this will unload by atime, atime is not updated by stat -- fix it

  XLOG(DBG2) << "forgetting stale inodes";
  // We have to destroy InodePtrs outside of the data lock. These hold all the
  // InodePtrs we created.
  std::vector<InodePtr> toClearFSRef;
  std::vector<InodePtr> justToHoldBeyondScopeOfLock;
  auto cutoff = std::chrono::system_clock::now() -
      config_->getEdenConfig()->postCheckoutDelayToUnloadInodes.getValue();
  auto cutoff_ts = folly::to<timespec>(cutoff);
  std::vector<InodeNumber> unloadedInodesToClearFSRef;

  {
    auto data = data_.wlock();

    for (auto& inode : data->unloadedInodes_) {
      XLOG(DBG9) << "Considering forgetting unloaded inode " << inode.first;
      if (inode.second.isUnlinked) {
        // We can't directly call decFsRefcountHelper here because it will
        // invalidate the iterator we are using for this for loop.
        unloadedInodesToClearFSRef.push_back(inode.first);
      } else {
        XLOG(DBG9) << "Not forgetting unloaded inode " << inode.first
                   << " because inode is still linked";
      }
    }

    for (auto& inodeNumber : unloadedInodesToClearFSRef) {
      auto inodePtr = decFsRefcountHelper(
          data,
          inodeNumber,
          /*count=*/0, // Doesn't matter what we set this to because we are
                       // going to clear the ref count.
          /*clearRefCount=*/true);
      XCHECK(!inodePtr)
          << "decFsRefcountHelper should not return a loaded inode that  "
          << "needs to be dereferenced for an inode we know to be unloaded.";
    }

    // we do this second because dereferencing a loaded inode will cause it to
    // be unloaded. Thus this will create lots of unloaded inodes. we don't want
    // to double decRef them, so we decref loaded inodes after unloaded ones.
    for (auto& inode : data->loadedInodes_) {
      XLOG(DBG9) << "Considering forgetting loaded inode " << inode.first;
      auto inodePtr = decFsRefcountHelper(
          data,
          inode.first,
          /*count=*/0, // Doesn't matter what we set this to because we are
                       // going to clear the ref count.
          /*clearRefCount=*/true);
      if (inodePtr) {
        auto unlinked = inodePtr->isUnlinked();
        if (unlinked &&
            inode.second->getMetadata().timestamps.atime < cutoff_ts) {
          XLOG(DBG9) << "Will forget loaded inode " << inode.first;
          toClearFSRef.push_back(inodePtr);
        } else {
          // even though we are not going to do anything with these inodes we
          // need to keep them around until we let go of the lock. It is not
          // safe to drop an inodePtr while holding the lock.
          if (!unlinked) {
            XLOG(DBG9) << "Not forgetting loaded inode " << inode.first
                       << " because it is still linked";
          } else {
            XLOG(DBG9) << "Not forgetting loaded inode " << inode.first
                       << " because it was referenced."
                       << durationStr(
                              config_->getEdenConfig()
                                  ->postCheckoutDelayToUnloadInodes.getValue() -
                              std::chrono::nanoseconds{
                                  inode.second->getMetadata()
                                      .timestamps.atime.toTimespec()
                                      .tv_nsec -
                                  cutoff_ts.tv_nsec})
                       << " ago";
          }
          justToHoldBeyondScopeOfLock.push_back(inodePtr);
        }
      }
    }
    numPeriodicallyUnloadedUnlinkedInodes_.fetch_add(
        toClearFSRef.size(), std::memory_order_relaxed);
  }

  for (auto& inodePtr : toClearFSRef) {
    XLOG(DBG7) << "forgetting NFS inode: " << inodePtr->getNodeId();
    inodePtr->clearFsRefcount();
  }
  XLOG(DBG2) << "forgetting stale inodes complete";
#endif
}

void InodeMap::setUnmounted() {
  auto data = data_.wlock();
  XDCHECK(!data->isUnmounted_);
  data->isUnmounted_ = true;
}

Future<SerializedInodeMap> InodeMap::shutdown(
    FOLLY_MAYBE_UNUSED bool doTakeover) {
  // Record that we are in the process of shutting down.
  auto future = Future<folly::Unit>::makeEmpty();
  {
    auto data = data_.wlock();
    XCHECK(!data->shutdownPromise.has_value())
        << "shutdown() invoked more than once on InodeMap for "
        << mount_->getPath();
    data->shutdownPromise.emplace(Promise<Unit>{});
    future = data->shutdownPromise->getFuture();

    XLOG(DBG3) << "starting InodeMap::shutdown: loadedCount="
               << data->loadedInodes_.size()
               << " unloadedCount=" << data->unloadedInodes_.size();
  }

  // If an error occurs during mount point initialization, shutdown() can be
  // called in some cases even if InodeMap::initialize() was never called.
  // Just return immediately in this case.
  if (!root_) {
    return folly::makeFuture(SerializedInodeMap{});
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
  // references, but outstanding FS references.)
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
      inodesToUnload.push_back(entry.second.getPtr());
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

  return std::move(future).thenValue([
#ifndef _WIN32
                                         this,
                                         doTakeover
#endif
  ](auto&&) {
#ifdef _WIN32
    // On Windows we don't have the takeover implemented yet, so we will return
    // from here.
    return SerializedInodeMap{};
#else
    // TODO: This check could occur after the loadedInodes_ assertion below to
    // maximize coverage of any invariants that are broken during shutdown.
    if (!doTakeover) {
      return SerializedInodeMap{};
    }

    auto data = data_.wlock();
    XLOG(DBG3)
        << "InodeMap::shutdown after releasing inodesToClear: loadedCount="
        << data->loadedInodes_.size()
        << " unloadedCount=" << data->unloadedInodes_.size();

    if (data->loadedInodes_.size() != 1) {
      EDEN_BUG() << "After InodeMap::shutdown() finished, "
                 << data->loadedInodes_.size()
                 << " inodes still loaded; they must all (except the root) "
                 << "have been unloaded for this to succeed!";
    }

    SerializedInodeMap result;
    result.unloadedInodes_ref()->reserve(data->unloadedInodes_.size());
    for (const auto& [inodeNumber, entry] : data->unloadedInodes_) {
      SerializedInodeMapEntry serializedEntry;

      XLOG(DBG5) << "  serializing unloaded inode " << inodeNumber
                 << " parent=" << entry.parent.get() << " name=" << entry.name;

      serializedEntry.inodeNumber_ref() = inodeNumber.get();
      serializedEntry.parentInode_ref() = entry.parent.get();
      serializedEntry.name_ref() = entry.name.stringPiece().str();
      serializedEntry.isUnlinked_ref() = entry.isUnlinked;
      serializedEntry.numFsReferences_ref() = entry.numFsReferences;
      if (entry.hash.has_value()) {
        serializedEntry.hash_ref() = entry.hash.value().asString();
      }
      // If entry.hash is empty, the inode is not materialized.
      serializedEntry.mode_ref() = entry.mode;

      result.unloadedInodes_ref()->emplace_back(std::move(serializedEntry));
    }

    return result;
#endif
  });
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

bool InodeMap::isInodeRemembered(InodeNumber ino) const {
  return data_.rlock()->unloadedInodes_.count(ino) > 0;
}

void InodeMap::onInodeUnreferenced(
    InodeBase* inode,
    ParentInodeInfo&& parentInfo) {
  XLOG(DBG8) << "inode " << inode->getNodeId()
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
  bool shuttingDown = data->shutdownPromise.has_value();
  XDCHECK(shuttingDown || inode != root_.get());
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
  } else if (parentInfo.isUnlinked() && inode->getFsRefcount() == 0) {
    // This inode has been unlinked and has no outstanding FS references.
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
      XCHECK(it != parentContents->entries.end());
      auto released = it->second.clearInode();
      XCHECK_EQ(inode, released);
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
    InodeBase* inode,
    TreeInode* parent,
    PathComponentPiece name,
    bool isUnlinked,
    const InodeMapLock& lock) {
  return unloadInode(inode, parent, name, isUnlinked, lock.data_);
}

void InodeMap::unloadInode(
    InodeBase* inode,
    TreeInode* parent,
    PathComponentPiece name,
    bool isUnlinked,
    const folly::Synchronized<Members>::LockedPtr& data) {
  // Call updateOverlayForUnload() to update the overlay and compute
  // if we need to remember an UnloadedInode entry.
  auto unloadedEntry =
      updateOverlayForUnload(inode, parent, name, isUnlinked, data);
  if (unloadedEntry) {
    // Insert the unloaded entry
    XLOG(DBG7) << "inserting unloaded map entry for inode "
               << inode->getNodeId();
    auto ret = data->unloadedInodes_.emplace(
        inode->getNodeId(), std::move(unloadedEntry.value()));
    XCHECK(ret.second);
  }

  auto numErased = data->loadedInodes_.erase(inode->getNodeId());
  XCHECK_EQ(numErased, 1u) << "inconsistent loaded inodes data: "
                           << inode->getLogPath();
  if (inode->getType() == dtype_t::Dir) {
    --data->numTreeInodes_;
  } else {
    --data->numFileInodes_;
  }
}

optional<InodeMap::UnloadedInode> InodeMap::updateOverlayForUnload(
    InodeBase* inode,
    TreeInode* parent,
    PathComponentPiece name,
    bool isUnlinked,
    const folly::Synchronized<Members>::LockedPtr& data) {
  auto fsCount = inode->getFsRefcount();
  if (isUnlinked && (data->isUnmounted_ || fsCount == 0)) {
    try {
      mount_->getOverlay()->removeOverlayData(inode->getNodeId());
    } catch (const std::exception& ex) {
      // If we fail to update the overlay log an error but do not propagate the
      // exception to our caller.  There is nothing else we can do to handle
      // this error.
      //
      // We still want to proceed unloading the inode normally in this case.
      //
      // The most common case where this can occur if the overlay file was
      // already corrupt (say, because of a hard reboot that did not sync
      // filesystem state).
      XLOG(ERR) << "error saving overlay state while unloading inode "
                << inode->getNodeId() << " (" << inode->getLogPath()
                << "): " << folly::exceptionStr(ex);
    }
  }

  // If the mount point has been unmounted, ignore any outstanding FS
  // refcounts on inodes that still existed before it was unmounted.
  // Everything is unreferenced by FS after an unmount operation, and we no
  // longer need to remember anything in the unloadedInodes_ map.
  if (data->isUnmounted_) {
    XLOG(DBG5) << "forgetting unreferenced inode " << inode->getNodeId()
               << " after unmount: " << inode->getLogPath();
    return std::nullopt;
  }

  // If the tree is unlinked and no longer referenced we can delete it from
  // the overlay and completely forget about it.
  if (isUnlinked && fsCount == 0) {
    XLOG(DBG5) << "forgetting unreferenced unlinked inode "
               << inode->getNodeId() << ": " << inode->getLogPath();
    return std::nullopt;
  }

  auto* asTree = dynamic_cast<TreeInode*>(inode);
  if (asTree) {
    // Normally, acquiring the tree's contents lock while the InodeMap members
    // lock is held violates our lock hierarchy. However, since this TreeInode
    // is being unloaded, nobody else can reference it right now, so the lock is
    // guaranteed not held. Therefore, it's not necessary to synchronize, and
    // the contents can be directly accessed here.
    auto& treeContents = asTree->getContents().unsafeGetUnlocked();

    // If the fs refcount is non-zero we have to remember this inode.
    if (fsCount > 0) {
      XLOG(DBG5) << "unloading tree inode " << inode->getNodeId()
                 << " with Fs refcount=" << fsCount << ": "
                 << inode->getLogPath();
      return UnloadedInode(
          parent, name, isUnlinked, treeContents.treeHash, fsCount);
    }

    // If any of this inode's childrens are in unloadedInodes_, then this
    // inode, as its parent, must not be forgotten.
    for (const auto& pair : treeContents.entries) {
      const auto& childName = pair.first;
      const auto& entry = pair.second;
      if (data->unloadedInodes_.count(entry.getInodeNumber())) {
        XLOG(DBG5) << "remembering inode " << asTree->getNodeId() << " ("
                   << asTree->getLogPath() << ") because its child "
                   << childName << " was remembered";
        return UnloadedInode(
            parent, name, isUnlinked, treeContents.treeHash, fsCount);
      }
    }
    return std::nullopt;
  } else {
    // We have to remember files only if their FS refcount is non-zero
    if (fsCount > 0) {
      XLOG(DBG5) << "unloading file inode " << inode->getNodeId()
                 << " with FS refcount=" << fsCount << ": "
                 << inode->getLogPath();
      auto* asFile = boost::polymorphic_downcast<FileInode*>(inode);
      return UnloadedInode(asFile, parent, name, isUnlinked, fsCount);
    } else {
      XLOG(DBG5) << "forgetting unreferenced file inode " << inode->getNodeId()
                 << " : " << inode->getLogPath();
      return std::nullopt;
    }
  }
}

bool InodeMap::shouldLoadChild(
    const TreeInode* parent,
    PathComponentPiece name,
    InodeNumber childInode,
    folly::Promise<InodePtr> promise) {
  auto data = data_.wlock();
  UnloadedInode* unloadedData{nullptr};
  auto iter = data->unloadedInodes_.find(childInode);
  if (iter == data->unloadedInodes_.end()) {
    InodeNumber parentNumber = parent->getNodeId();
    auto newUnloadedData = UnloadedInode(parentNumber, name);
    auto ret =
        data->unloadedInodes_.emplace(childInode, std::move(newUnloadedData));
    XDCHECK(ret.second);
    unloadedData = &ret.first->second;
  } else {
    unloadedData = &iter->second;
  }

  bool isFirstPromise = unloadedData->promises.empty();

  // Add the promise to the existing list for this inode.
  unloadedData->promises.push_back(std::move(promise));

  // If this is the very first promise then tell the caller they need
  // to start the load operation.  Otherwise someone else (whoever added the
  // first promise) has already started loading the inode.
  return isFirstPromise;
}

void InodeMap::inodeCreated(const InodePtr& inode) {
  XLOG(DBG4) << "created new inode " << inode->getNodeId() << ": "
             << inode->getLogPath();
  auto data = data_.wlock();
  insertLoadedInode(data, inode.get());
}

void InodeMap::recordPeriodicInodeUnload(size_t numInodesToUnload) {
  numPeriodicallyUnloadedLinkedInodes_.fetch_add(
      numInodesToUnload, std::memory_order_relaxed);
}

InodeMap::InodeCounts InodeMap::getInodeCounts() const {
  InodeCounts counts;
  auto data = data_.rlock();
  XDCHECK_EQ(
      data->numTreeInodes_ + data->numFileInodes_, data->loadedInodes_.size());
  counts.treeCount = data->numTreeInodes_;
  counts.fileCount = data->numFileInodes_;
  counts.unloadedInodeCount = data->unloadedInodes_.size();
  counts.periodicUnlinkedUnloadInodeCount =
      numPeriodicallyUnloadedUnlinkedInodes_.load(std::memory_order_relaxed);
  counts.periodicLinkedUnloadInodeCount =
      numPeriodicallyUnloadedLinkedInodes_.load(std::memory_order_relaxed);
  return counts;
}

std::vector<InodeNumber> InodeMap::getReferencedInodes() const {
  std::vector<InodeNumber> inodes;
  {
    auto data = data_.rlock();

    for (auto& kv : data->loadedInodes_) {
      auto& loadedInode = kv.second;

      inodes.push_back(loadedInode->getNodeId());
    }

    for (const auto& [ino, unloadedInode] : data->unloadedInodes_) {
      if (unloadedInode.numFsReferences > 0) {
        inodes.push_back(ino);
      }
    }
  }

  return inodes;
}
} // namespace facebook::eden
