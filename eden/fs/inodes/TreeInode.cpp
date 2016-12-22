/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInode.h"

#include "EdenDispatcher.h"
#include "EdenMount.h"
#include "FileHandle.h"
#include "FileInode.h"
#include "InodeMap.h"
#include "Overlay.h"
#include "TreeInodeDirHandle.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/fuse/RequestData.h"
#include "eden/utils/Bug.h"
#include "eden/utils/PathFuncs.h"

using folly::Future;
using folly::makeFuture;

namespace facebook {
namespace eden {

TreeInode::TreeInode(
    fuse_ino_t ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    Entry* entry,
    std::unique_ptr<Tree>&& tree)
    : TreeInode(ino, parent, name, entry, buildDirFromTree(tree.get())) {}

TreeInode::TreeInode(
    fuse_ino_t ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    Entry* entry,
    Dir&& dir)
    : InodeBase(ino, parent, name),
      contents_(std::move(dir)),
      entry_(entry),
      parent_(parent->getNodeId()) {
  DCHECK_NE(ino, FUSE_ROOT_ID);
  DCHECK_NOTNULL(entry_);
}

TreeInode::TreeInode(EdenMount* mount, std::unique_ptr<Tree>&& tree)
    : TreeInode(mount, buildDirFromTree(tree.get())) {}

TreeInode::TreeInode(EdenMount* mount, Dir&& dir)
    : InodeBase(mount),
      contents_(std::move(dir)),
      entry_(nullptr),
      parent_(FUSE_ROOT_ID) {}

TreeInode::~TreeInode() {}

folly::Future<fusell::Dispatcher::Attr> TreeInode::getattr() {
  return getAttrLocked(&*contents_.rlock());
}

fusell::Dispatcher::Attr TreeInode::getAttrLocked(const Dir* contents) {
  fusell::Dispatcher::Attr attr(getMount()->getMountPoint());

  attr.st.st_mode = S_IFDIR | 0755;
  attr.st.st_ino = getNodeId();
  // TODO: set atime, mtime, and ctime

  // For directories, nlink is the number of entries including the
  // "." and ".." links.
  attr.st.st_nlink = contents->entries.size() + 2;
  return attr;
}

folly::Future<InodePtr> TreeInode::getChildByName(
    PathComponentPiece namepiece) {
  return getOrLoadChild(namepiece);
}

Future<InodePtr> TreeInode::getOrLoadChild(PathComponentPiece name) {
  folly::Optional<folly::Future<InodePtr>> inodeLoadFuture;
  folly::Optional<folly::Future<InodePtr>> returnFuture;
  fuse_ino_t childNumber;
  {
    auto contents = contents_.wlock();
    auto iter = contents->entries.find(name);
    if (iter == contents->entries.end()) {
      VLOG(5) << "attempted to load non-existent entry \"" << name << "\" in "
              << getLogPath();
      return makeFuture<InodePtr>(InodeError(ENOENT, inodePtrFromThis(), name));
    }

    // Check to see if the entry is already loaded
    auto& entryPtr = iter->second;
    if (entryPtr->inode) {
      return makeFuture(entryPtr->inode->shared_from_this());
    }

    // The entry is not loaded yet.  Ask the InodeMap about the entry.
    // The InodeMap will tell us if this inode is already in the process of
    // being loaded, or if we need to start loading it now.
    folly::Promise<InodePtr> promise;
    returnFuture = promise.getFuture();
    if (getInodeMap()->shouldLoadChild(
            this, name, std::move(promise), &childNumber)) {
      // The inode is not already being loaded.  We have to start loading it
      // now.
      auto loadFuture =
          startLoadingInodeNoThrow(entryPtr.get(), name, childNumber);
      if (loadFuture.isReady() && loadFuture.hasValue()) {
        // If we finished loading the inode immediately, just call
        // InodeMap::inodeLoadComplete() now, since we still have the data_
        // lock.
        auto childInode = loadFuture.get();
        entryPtr->inode = childInode.get();
        getInodeMap()->inodeLoadComplete(childInode);
      } else {
        inodeLoadFuture = std::move(loadFuture);
      }
    }
  }

  if (inodeLoadFuture) {
    registerInodeLoadComplete(inodeLoadFuture.value(), name, childNumber);
  }

  return std::move(returnFuture).value();
}

Future<TreeInodePtr> TreeInode::getOrLoadChildTree(PathComponentPiece name) {
  return getOrLoadChild(name).then([](InodePtr child) {
    auto treeInode = std::dynamic_pointer_cast<TreeInode>(child);
    if (!treeInode) {
      return makeFuture<TreeInodePtr>(InodeError(ENOTDIR, child));
    }
    return makeFuture(treeInode);
  });
}

fuse_ino_t TreeInode::getChildInodeNumber(PathComponentPiece name) {
  auto contents = contents_.wlock();
  auto iter = contents->entries.find(name);
  if (iter == contents->entries.end()) {
    throw InodeError(ENOENT, inodePtrFromThis(), name);
  }

  auto& ent = iter->second;
  if (ent->inode) {
    return ent->inode->getNodeId();
  }

  // TODO: We should probably track unloaded inode numbers directly in the
  // TreeInode, rather than in the separate unloadedInodesReverse_ map in
  // InodeMap.
  return getInodeMap()->getOrAllocateUnloadedInodeNumber(this, name);
}

void TreeInode::loadChildInode(PathComponentPiece name, fuse_ino_t number) {
  folly::Optional<folly::Future<InodePtr>> future;
  {
    auto contents = contents_.rlock();
    auto iter = contents->entries.find(name);
    if (iter == contents->entries.end()) {
      auto bug = EDEN_BUG() << "InodeMap requested to load inode " << number
                            << ", but there is no entry named \"" << name
                            << "\" in " << getNodeId();
      getInodeMap()->inodeLoadFailed(number, bug.toException());
      return;
    }

    auto& entryPtr = iter->second;
    // InodeMap makes sure to only try loading each inode once, so this entry
    // should not already be loaded.
    if (entryPtr->inode != nullptr) {
      auto bug = EDEN_BUG() << "InodeMap requested to load inode " << number
                            << "(" << name << " in " << getNodeId()
                            << "), which is already loaded";
      // Call inodeLoadFailed().  (Arguably we could call inodeLoadComplete()
      // if the existing inode has the same number as the one we were requested
      // to load.  However, it seems more conservative to just treat this as
      // failed and fail pending promises waiting on this inode.  This may
      // cause problems for anyone trying to access this child inode in the
      // future, but at least it shouldn't damage the InodeMap data structures
      // any further.)
      getInodeMap()->inodeLoadFailed(number, bug.toException());
      return;
    }

    future = startLoadingInodeNoThrow(entryPtr.get(), name, number);
  }
  registerInodeLoadComplete(future.value(), name, number);
}

void TreeInode::registerInodeLoadComplete(
    folly::Future<InodePtr>& future,
    PathComponentPiece name,
    fuse_ino_t number) {
  // This method should never be called with the data_ lock held.
  // If the future is already ready we will try to acquire the data_ lock now.
  future
      .then([ self = inodePtrFromThis(), childName = PathComponent{name} ](
          const InodePtr& childInode) {
        auto contents = self->contents_.wlock();
        auto iter = contents->entries.find(childName);
        if (iter == contents->entries.end()) {
          // This probably shouldn't ever happen.
          // We should ensure that the child inode is loaded first before
          // renaming or unlinking it.
          LOG(ERROR) << "child " << childName << " in " << self->getLogPath()
                     << " removed before it finished loading";
          throw InodeError(
              ENOENT, self, childName, "inode removed before loading finished");
        }
        iter->second->inode = childInode.get();
        // Make sure that we are still holding the contents_ lock when
        // calling inodeLoadComplete()
        self->getInodeMap()->inodeLoadComplete(childInode);
      })
      .onError([ self = inodePtrFromThis(), number ](
          const folly::exception_wrapper& ew) {
        self->getInodeMap()->inodeLoadFailed(number, ew);
      });
}

Future<InodePtr> TreeInode::startLoadingInodeNoThrow(
    Entry* entry,
    PathComponentPiece name,
    fuse_ino_t number) noexcept {
  // The callers of startLoadingInodeNoThrow() need to make sure that they
  // always call InodeMap::inodeLoadComplete() or InodeMap::inodeLoadFailed()
  // afterwards.
  //
  // It simplifies their logic to guarantee that we never throw an exception,
  // and always return a Future object.  Therefore we simply wrap
  // startLoadingInode() and convert any thrown exceptions into Future.
  try {
    return startLoadingInode(entry, name, number);
  } catch (const std::exception& ex) {
    // It's possible that makeFuture() itself could throw, but this only
    // happens on out of memory, in which case the whole process is pretty much
    // hosed anyway.
    return makeFuture<InodePtr>(
        folly::exception_wrapper{std::current_exception(), ex});
  }
}

Future<InodePtr> TreeInode::startLoadingInode(
    Entry* entry,
    PathComponentPiece name,
    fuse_ino_t number) {
  VLOG(5) << "starting to load inode " << number << ": " << getLogPath()
          << " / \"" << name << "\"";
  DCHECK(entry->inode == nullptr);
  if (!S_ISDIR(entry->mode)) {
    // If this is a file we can just go ahead and create it now;
    // we don't need to load anything else.
    //
    // Eventually we may want to go ahead start loading some of the blob data
    // now, but we don't have to wait for it to be ready before marking the
    // inode loaded.
    auto inode = std::make_shared<FileInode>(
        number,
        std::static_pointer_cast<TreeInode>(shared_from_this()),
        name,
        entry);
    return makeFuture(inode);
  }

  // TODO:
  // - Always load the Tree if this entry has one.  This is needed so we can
  //   compute diffs from the current commit state.  This will simplify
  //   Dirstate computation.
  // - The ObjectStore APIs should be updated to return a Future when loading
  //   the Tree, since this can potentially be a costly operation.
  // - We can potentially start loading the overlay data in parallel with
  //   loading the Tree.

  if (!entry->materialized && entry->hash) {
    return getStore()->getTreeFuture(entry->hash.value()).then([
      self = std::static_pointer_cast<TreeInode>(shared_from_this()),
      childName = PathComponent{name},
      entry,
      number
    ](std::unique_ptr<Tree> tree) {
      return std::make_shared<TreeInode>(
          number, self, childName, entry, std::move(tree));
    });
  }

  // No corresponding TreeEntry, this exists only in the overlay.
  //
  // TODO: We should probably require that an inode be loaded before it can be
  // unlinked or renamed.  Otherwise it seems like there are race conditions
  // here between computing the path to the child's materialized data file and
  // the time when we actually open it.
  auto targetName = getPathBuggy() + name;
  auto overlayDir = getOverlay()->loadOverlayDir(targetName);
  DCHECK(overlayDir) << "missing overlay for " << targetName;
  return std::make_shared<TreeInode>(
      number, inodePtrFromThis(), name, entry, std::move(overlayDir.value()));
}

fuse_ino_t TreeInode::getParent() const {
  return parent_;
}

fuse_ino_t TreeInode::getInode() const {
  return getNodeId();
}

folly::Future<std::shared_ptr<fusell::DirHandle>> TreeInode::opendir(
    const struct fuse_file_info&) {
  return std::make_shared<TreeInodeDirHandle>(inodePtrFromThis());
}

/* If we don't yet have an overlay entry for this portion of the tree,
 * populate it from the Tree.  In order to materialize a dir we have
 * to also materialize its parents. */
void TreeInode::materializeDirAndParents() {
  if (contents_.rlock()->materialized) {
    // Already materialized, all done!
    return;
  }

  // Ensure that our parent(s) are materialized.  We can't go higher
  // than the root inode though.
  if (getNodeId() != FUSE_ROOT_ID) {
    auto parentInode = getParentBuggy();
    parentInode->materializeDirAndParents();
  }

  // Atomically, wrt. to concurrent callers, cause the materialized flag
  // to be set to true both for this directory and for our entry in the
  // parent directory in the in-memory state.
  bool updateParent = contents_.withWLockPtr([&](auto wlock) {
    if (wlock->materialized) {
      // Someone else materialized it in the meantime
      return false;
    }

    auto myname = this->getPathBuggy();
    auto overlay = this->getOverlay();
    auto dirPath = overlay->getContentDir() + myname;
    if (::mkdir(dirPath.c_str(), 0755) != 0 && errno != EEXIST) {
      folly::throwSystemError("while materializing, mkdir: ", dirPath);
    }
    wlock->materialized = true;
    overlay->saveOverlayDir(myname, &*wlock);

    if (entry_ && !entry_->materialized) {
      entry_->materialized = true;
      return true;
    }

    return false;
  });

  // If we just set materialized on the entry, we need to arrange for that
  // state to be saved to disk.  This is not atomic wrt. to the property
  // change, but definitely does not have a lock-order-acquisition deadlock.
  // This means that there is a small window of time where our in-memory and
  // on-disk state for the overlay are not in sync.
  if (updateParent) {
    // FIXME: Overlay file paths should be based on our inode number,
    // not on our path.
    auto parentInode = getParentBuggy();
    auto parentName = parentInode->getPathBuggy();
    getOverlay()->saveOverlayDir(parentName, &*parentInode->contents_.wlock());
  }
}

TreeInode::Dir TreeInode::buildDirFromTree(const Tree* tree) {
  // Now build out the Dir based on what we know.
  Dir dir;
  if (!tree) {
    // There's no associated Tree, so we have to persist this to the
    // overlay storage area
    dir.materialized = true;
    return dir;
  }

  dir.treeHash = tree->getHash();
  for (const auto& treeEntry : tree->getTreeEntries()) {
    Entry entry;

    entry.hash = treeEntry.getHash();
    entry.mode = treeEntry.getMode();

    dir.entries.emplace(
        treeEntry.getName(), std::make_unique<Entry>(std::move(entry)));
  }
  return dir;
}

folly::Future<TreeInode::CreateResult>
TreeInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Figure out the relative path to this inode.
  auto myname = getPathBuggy();

  // Compute the effective name of the node they want to create.
  auto targetName = myname + name;
  std::shared_ptr<FileHandle> handle;
  FileInodePtr inode;

  materializeDirAndParents();

  auto filePath = getOverlay()->getContentDir() + targetName;

  // We need to scope the write lock as the getattr call below implicitly
  // wants to acquire a read lock.
  contents_.withWLock([&](auto& contents) {
    // Since we will move this file into the underlying file data, we
    // take special care to ensure that it is opened read-write
    folly::File file(
        filePath.c_str(),
        O_RDWR | O_CREAT | (flags & ~(O_RDONLY | O_WRONLY)),
        0600);

    // Record the new entry
    auto& entry = contents.entries[name];
    entry = std::make_unique<Entry>();
    entry->materialized = true;

    struct stat st;
    folly::checkUnixError(::fstat(file.fd(), &st));
    entry->mode = st.st_mode;

    // Generate an inode number for this new entry.
    auto* inodeMap = this->getInodeMap();
    auto childNumber = inodeMap->allocateInodeNumber();

    // build a corresponding FileInode
    inode = std::make_shared<FileInode>(
        childNumber,
        this->inodePtrFromThis(),
        name,
        entry.get(),
        std::move(file));
    entry->inode = inode.get();
    inodeMap->inodeCreated(inode);

    // The kernel wants an open operation to return the inode,
    // the file handle and some attribute information.
    // Let's open a file handle now.
    handle = inode->finishCreate();

    this->getOverlay()->saveOverlayDir(myname, &contents);
  });

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  // Now that we have the file handle, let's look up the attributes.
  auto getattrResult = handle->getattr();
  return getattrResult.then(
      [ =, handle = std::move(handle) ](fusell::Dispatcher::Attr attr) mutable {
        CreateResult result(getMount()->getMountPoint());

        // Return all of the results back to the kernel.
        result.inode = inode;
        result.file = std::move(handle);
        result.attr = attr;

        return result;
      });
}

folly::Future<fuse_entry_param> TreeInode::symlink(
    PathComponentPiece /* name */,
    folly::StringPiece /* contents */) {
  // TODO
  FUSELL_NOT_IMPL();
}

TreeInodePtr TreeInode::mkdir(PathComponentPiece name, mode_t mode) {
  // Figure out the relative path to this inode.
  auto myname = getPathBuggy();
  auto targetName = myname + name;
  // Compute the effective name of the node they want to create.
  materializeDirAndParents();

  TreeInodePtr newChild;
  contents_.withWLock([&](auto& contents) {
    auto entIter = contents.entries.find(name);
    if (entIter != contents.entries.end()) {
      throw InodeError(EEXIST, this->inodePtrFromThis(), name);
    }
    auto overlay = this->getOverlay();

    auto dirPath = overlay->getContentDir() + targetName;

    folly::checkUnixError(
        ::mkdir(dirPath.c_str(), mode), "mkdir: ", dirPath, " mode=", mode);

    // We succeeded, let's update our state
    struct stat st;
    folly::checkUnixError(::lstat(dirPath.c_str(), &st));

    auto entry = std::make_unique<Entry>();
    entry->mode = st.st_mode;
    entry->materialized = true;

    // Create the overlay entry for this dir
    Dir emptyDir;
    emptyDir.materialized = true;
    overlay->saveOverlayDir(targetName, &emptyDir);

    // Create the TreeInode
    auto* inodeMap = this->getInodeMap();
    auto childNumber = inodeMap->allocateInodeNumber();
    newChild = std::make_shared<TreeInode>(
        childNumber,
        this->inodePtrFromThis(),
        name,
        entry.get(),
        std::move(emptyDir));
    entry->inode = newChild.get();
    inodeMap->inodeCreated(newChild);

    contents.entries.emplace(name, std::move(entry));
    overlay->saveOverlayDir(myname, &contents);
  });

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return newChild;
}

folly::Future<folly::Unit> TreeInode::unlink(PathComponentPiece name) {
  // TODO: We should grab the mountpoint-wide rename lock here.

  // Compute the full name of the node they want to remove.
  auto myname = getPathBuggy();
  auto targetName = myname + name;

  // Check pre-conditions with a read lock before we materialize anything
  // in case we're processing spurious unlink for a non-existent entry;
  // we don't want to materialize part of a tree if we're not actually
  // going to do any work in it.
  contents_.withRLock([&](const auto& contents) {
    auto entIter = contents.entries.find(name);
    if (entIter == contents.entries.end()) {
      throw InodeError(ENOENT, this->inodePtrFromThis(), name);
    }
    if (S_ISDIR(entIter->second->mode)) {
      throw InodeError(EISDIR, this->inodePtrFromThis(), name);
    }
  });

  materializeDirAndParents();

  contents_.withWLock([&](auto& contents) {
    // Re-check the pre-conditions in case we raced
    auto entIter = contents.entries.find(name);
    if (entIter == contents.entries.end()) {
      throw InodeError(ENOENT, this->inodePtrFromThis(), name);
    }

    auto& ent = entIter->second;
    if (S_ISDIR(ent->mode)) {
      throw InodeError(EISDIR, this->inodePtrFromThis(), name);
    }

    auto overlay = this->getOverlay();

    if (ent->materialized) {
      auto filePath = overlay->getContentDir() + targetName;
      folly::checkUnixError(::unlink(filePath.c_str()), "unlink: ", filePath);
    }
    // If the child inode in question is loaded, inform it that it has been
    // unlinked.
    if (ent->inode) {
      ent->inode->markUnlinked();
    }

    // And actually remove it
    contents.entries.erase(entIter);
    overlay->saveOverlayDir(myname, &contents);
  });

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return folly::Unit{};
}

folly::Future<folly::Unit> TreeInode::rmdir(PathComponentPiece name) {
  // TODO: We should probably grab the mountpoint-wide rename lock here,
  // and hold it during the child lookup.  This would avoid having to retry.
  return getOrLoadChildTree(name).then(
      [ self = inodePtrFromThis(),
        childName = PathComponent{name} ](const TreeInodePtr& child) {
        return self->rmdirImpl(childName, child, 1);
      });
}

folly::Future<folly::Unit> TreeInode::rmdirImpl(
    PathComponent name,
    TreeInodePtr child,
    unsigned int attemptNum) {
  // Verify that the child directory is empty before we materialize ourself
  {
    auto childContents = child->contents_.rlock();
    if (!childContents->entries.empty()) {
      throw InodeError(ENOTEMPTY, child);
    }
  }

  materializeDirAndParents();

  // Lock our contents in write mode.
  // We will hold it for the duration of the unlink.
  auto targetName = getPathBuggy() + name;
  {
    auto contents = contents_.wlock();

    // Make sure that this name still corresponds to the child inode we just
    // looked up.
    auto entIter = contents->entries.find(name);
    if (entIter == contents->entries.end()) {
      throw InodeError(ENOENT, inodePtrFromThis(), name);
    }
    auto& ent = entIter->second;
    if (ent->inode != child.get()) {
      // This child was replaced since the rmdir attempt started.
      if (ent->inode == nullptr) {
        constexpr unsigned int kMaxRmdirRetries = 3;
        if (attemptNum > kMaxRmdirRetries) {
          throw InodeError(
              EIO,
              inodePtrFromThis(),
              name,
              "directory was removed/renamed after "
              "rmdir() started");
        }
        contents.unlock();
        // Note that we intentially create childFuture() in a separate
        // statement before calling then() on it, since we std::move()
        // the name into the lambda capture for then().
        //
        // Pre-C++17 this has undefined behavior if they are both in the same
        // statement: argument evaluation order is undefined, so we could
        // create the lambda (and invalidate name) before calling
        // getOrLoadChildTree(name).  C++17 fixes this order to guarantee that
        // the left side of "." will always get evaluated before the right
        // side.
        auto childFuture = getOrLoadChildTree(name);
        return childFuture.then([
          self = inodePtrFromThis(),
          childName = PathComponent{std::move(name)},
          attemptNum
        ](const TreeInodePtr& child) {
          return self->rmdirImpl(childName, child, attemptNum + 1);
        });
      } else {
        // Just update to point to the current child
        InodePtr childGeneric = ent->inode->shared_from_this();
        child = std::dynamic_pointer_cast<TreeInode>(childGeneric);
        if (!child) {
          throw InodeError(ENOTDIR, childGeneric);
        }
      }
    }

    // Lock the child contents, and make sure they are still empty
    auto childContents = child->contents_.rlock();
    if (!childContents->entries.empty()) {
      throw InodeError(ENOTEMPTY, child);
    }

    // Now we can do the rmdir
    auto overlay = this->getOverlay();
    if (ent->materialized) {
      auto dirPath = overlay->getContentDir() + targetName;
      folly::checkUnixError(::rmdir(dirPath.c_str()), "rmdir: ", dirPath);
    }
    if (ent->inode) {
      ent->inode->markUnlinked();
    }

    // And actually remove it
    contents->entries.erase(entIter);
    overlay->saveOverlayDir(getPathBuggy(), &*contents);
    overlay->removeOverlayDir(targetName);
  }

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return folly::Unit{};
}

void TreeInode::renameHelper(
    Dir* sourceContents,
    PathComponentPiece sourceName,
    TreeInodePtr destParent,
    Dir* destContents,
    PathComponentPiece destName) {
  auto sourceEntIter = sourceContents->entries.find(sourceName);
  if (sourceEntIter == sourceContents->entries.end()) {
    throw InodeError(ENOENT, inodePtrFromThis(), sourceName);
  }

  auto destEntIter = destContents->entries.find(destName);

  if (mode_to_dtype(sourceEntIter->second->mode) == dtype_t::Dir &&
      destEntIter != destContents->entries.end()) {
    // When renaming a directory, the destination must either not exist or
    // it must be an empty directory
    if (mode_to_dtype(destEntIter->second->mode) != dtype_t::Dir) {
      throw InodeError(ENOTDIR, destParent, destName);
    }

    // If the directory is loaded, check to see if it contains anything
    if (destEntIter->second->inode != nullptr) {
      auto destDir = dynamic_cast<TreeInode*>(destEntIter->second->inode);
      if (!destDir) {
        throw InodeError(
            EIO,
            destParent,
            destName,
            "inconsistency between contents and inodes objects");
      }

      if (!destDir->contents_.rlock()->entries.empty()) {
        throw InodeError(ENOTEMPTY, destParent, destName);
      }
    } else {
      // This directory is not currently loaded.
      // This means that it cannot be materialized, so it only contains the
      // contents from its Tree.  We don't ever track empty Trees, so therefore
      // the directory cannot be empty.
      throw InodeError(ENOTEMPTY, destParent, destName);
    }
  }

  // If we haven't actually materialized it yet, the rename() call will
  // fail.  So don't try that.
  if (sourceEntIter->second->materialized) {
    auto contentDir = getOverlay()->getContentDir();
    auto absoluteSourcePath = contentDir + getPathBuggy() + sourceName;
    auto absoluteDestPath = contentDir + destParent->getPathBuggy() + destName;
    folly::checkUnixError(
        ::rename(absoluteSourcePath.c_str(), absoluteDestPath.c_str()),
        "rename ",
        absoluteSourcePath,
        " to ",
        absoluteDestPath,
        " failed");
  }

  // Success.
  // Update the destination with the source data (this copies in the hash if
  // it happens to be set).
  auto& destEnt = destContents->entries[destName];
  // Note: sourceEntIter may have been invalidated by the line above in the
  // case that the source and destination dirs are the same.  We need to
  // recompute that iterator now to be safe.
  sourceEntIter = sourceContents->entries.find(sourceName);

  if (destEnt && destEnt->inode) {
    destEnt->inode->markUnlinked();
  }

  // We want to move in the data from the source.
  destEnt = std::move(sourceEntIter->second);

  if (destEnt->inode) {
    destEnt->inode->updateLocation(destParent, destName);
  }

  // Now remove the source information
  sourceContents->entries.erase(sourceEntIter);
}

folly::Future<folly::Unit> TreeInode::rename(
    PathComponentPiece name,
    TreeInodePtr newParent,
    PathComponentPiece newName) {
  // TODO: Grab a mountpoint-wide rename lock so that no other rename
  // operations can happen while we are running.

  auto myPath = getPathBuggy();
  auto destDirPath = newParent->getPathBuggy();

  // Check pre-conditions with a read lock before we materialize anything
  // in case we're processing spurious rename for a non-existent entry;
  // we don't want to materialize part of a tree if we're not actually
  // going to do any work in it.
  // There are some more complex pre-conditions that we'd like to check
  // before materializing, but we cannot do so in a race free manner
  // without locking each of the associated objects.  The existence
  // check is sufficient to avoid the majority of the potentially
  // wasted effort.
  contents_.withRLock([&](const auto& contents) {
    auto entIter = contents.entries.find(name);
    if (entIter == contents.entries.end()) {
      throw InodeError(ENOENT, this->inodePtrFromThis(), name);
    }
  });

  materializeDirAndParents();

  // Can't use SYNCHRONIZED_DUAL for both cases, as we'd self-deadlock by trying
  // to wlock the same thing twice
  if (newParent.get() == this) {
    contents_.withWLock([&](auto& contents) {
      this->renameHelper(&contents, name, newParent, &contents, newName);
      this->getOverlay()->saveOverlayDir(myPath, &contents);
    });
  } else {
    newParent->materializeDirAndParents();

    // TODO: SYNCHRONIZED_DUAL is not the correct locking order to use here.
    // We need to figure out if the source and dest are ancestors/children of
    // each other.  If so we have to lock the ancestor first.  Otherwise we may
    // deadlock with other operations that always acquire parent directory
    // locks first (e.g., rmdir())
    SYNCHRONIZED_DUAL(
        sourceContents, contents_, destContents, newParent->contents_) {
      renameHelper(&sourceContents, name, newParent, &destContents, newName);
      getOverlay()->saveOverlayDir(myPath, &sourceContents);
      getOverlay()->saveOverlayDir(destDirPath, &destContents);
    }
  }

  auto sourceName = myPath + name;
  auto targetName = destDirPath + newName;
  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{sourceName, targetName}));
  return folly::Unit{};
}

InodeMap* TreeInode::getInodeMap() const {
  return getMount()->getInodeMap();
}

ObjectStore* TreeInode::getStore() const {
  return getMount()->getObjectStore();
}

const std::shared_ptr<Overlay>& TreeInode::getOverlay() const {
  return getMount()->getOverlay();
}

void TreeInode::performCheckout(const Hash& hash) {
  throw std::runtime_error("not yet implemented");
}
}
}
