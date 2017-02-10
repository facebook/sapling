/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInode.h"

#include <boost/polymorphic_cast.hpp>
#include <folly/futures/Future.h>
#include <vector>
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
using folly::StringPiece;
using folly::Unit;
using std::make_unique;
using std::unique_ptr;

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
    : InodeBase(ino, parent, name), contents_(std::move(dir)), entry_(entry) {
  DCHECK_NE(ino, FUSE_ROOT_ID);
  DCHECK_NOTNULL(entry_);
}

TreeInode::TreeInode(EdenMount* mount, std::unique_ptr<Tree>&& tree)
    : TreeInode(mount, buildDirFromTree(tree.get())) {}

TreeInode::TreeInode(EdenMount* mount, Dir&& dir)
    : InodeBase(mount), contents_(std::move(dir)), entry_(nullptr) {}

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
  folly::Optional<Future<unique_ptr<InodeBase>>> inodeLoadFuture;
  folly::Optional<Future<InodePtr>> returnFuture;
  InodePtr childInodePtr;
  InodeMap::PromiseVector promises;
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
      return makeFuture<InodePtr>(InodePtr::newPtrLocked(entryPtr->inode));
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
        promises = getInodeMap()->inodeLoadComplete(childInode.get());
        childInodePtr = InodePtr::newPtrLocked(childInode.release());
      } else {
        inodeLoadFuture = std::move(loadFuture);
      }
    }
  }

  if (inodeLoadFuture) {
    registerInodeLoadComplete(inodeLoadFuture.value(), name, childNumber);
  } else {
    for (auto& promise : promises) {
      promise.setValue(childInodePtr);
    }
  }

  return std::move(returnFuture).value();
}

Future<TreeInodePtr> TreeInode::getOrLoadChildTree(PathComponentPiece name) {
  return getOrLoadChild(name).then([](InodePtr child) {
    auto treeInode = child.asTreePtrOrNull();
    if (!treeInode) {
      return makeFuture<TreeInodePtr>(InodeError(ENOTDIR, child));
    }
    return makeFuture(treeInode);
  });
}

namespace {
/**
 * A helper class for performing a recursive path lookup.
 *
 * If needed we could probably optimize this more in the future.  As-is we are
 * likely performing a lot of avoidable memory allocations to bind and set
 * Future callbacks at each stage.  This should be possible to implement with
 * only a single allocation up front (but we might not be able to achieve that
 * using the Futures API, we might have to create more custom callback API).
 */
class LookupProcessor {
 public:
  explicit LookupProcessor(RelativePathPiece path) : path_{path} {}

  Future<InodePtr> next(TreeInodePtr tree) {
    auto pathStr = path_.stringPiece();
    DCHECK_LT(pathIndex_, pathStr.size());
    auto endIdx = pathStr.find(kDirSeparator, pathIndex_);
    if (endIdx == StringPiece::npos) {
      auto name = StringPiece{pathStr.data() + pathIndex_, pathStr.end()};
      return tree->getOrLoadChild(PathComponentPiece{name});
    }

    auto name =
        StringPiece{pathStr.data() + pathIndex_, pathStr.data() + endIdx};
    pathIndex_ = endIdx + 1;
    return tree->getOrLoadChildTree(PathComponentPiece{name})
        .then(&LookupProcessor::next, this);
  }

 private:
  RelativePath path_;
  size_t pathIndex_{0};
};
}

Future<InodePtr> TreeInode::getChildRecursive(RelativePathPiece path) {
  auto pathStr = path.stringPiece();
  if (pathStr.empty()) {
    return makeFuture<InodePtr>(InodePtr::newPtrFromExisting(this));
  }

  auto processor = std::make_unique<LookupProcessor>(path);
  auto future = processor->next(TreeInodePtr::newPtrFromExisting(this));
  // This ensure() callback serves to hold onto the unique_ptr,
  // and makes sure it only gets destroyed when the future is finally resolved.
  return future.ensure([p = std::move(processor)]() mutable { p.reset(); });
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
  folly::Optional<folly::Future<unique_ptr<InodeBase>>> future;
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
    folly::Future<unique_ptr<InodeBase>>& future,
    PathComponentPiece name,
    fuse_ino_t number) {
  // This method should never be called with the contents_ lock held.  If the
  // future is already ready we will try to acquire the contents_ lock now.
  future
      .then([ self = inodePtrFromThis(), childName = PathComponent{name} ](
          unique_ptr<InodeBase> && childInode) {
        self->inodeLoadComplete(childName, std::move(childInode));
      })
      .onError([ self = inodePtrFromThis(), number ](
          const folly::exception_wrapper& ew) {
        self->getInodeMap()->inodeLoadFailed(number, ew);
      });
}

void TreeInode::inodeLoadComplete(
    PathComponentPiece childName,
    std::unique_ptr<InodeBase> childInode) {
  InodeMap::PromiseVector promises;

  {
    auto contents = contents_.wlock();
    auto iter = contents->entries.find(childName);
    if (iter == contents->entries.end()) {
      // This shouldn't ever happen.
      // The rename(), unlink(), and rmdir() code should always ensure
      // the child inode in question is loaded before removing or renaming
      // it.  (We probably could allow renaming/removing unloaded inodes,
      // but the loading process would have to be significantly more
      // complicated to deal with this, both here and in the parent lookup
      // process in InodeMap::lookupInode().)
      LOG(ERROR) << "child " << childName << " in " << getLogPath()
                 << " removed before it finished loading";
      throw InodeError(
          ENOENT,
          inodePtrFromThis(),
          childName,
          "inode removed before loading finished");
    }
    iter->second->inode = childInode.get();
    // Make sure that we are still holding the contents_ lock when
    // calling inodeLoadComplete().  This ensures that no-one can look up
    // the inode by name before it is also available in the InodeMap.
    // However, we must wait to fulfill pending promises until after
    // releasing our lock.
    promises = getInodeMap()->inodeLoadComplete(childInode.get());
  }

  // Fulfill all of the pending promises after releasing our lock
  auto inodePtr = InodePtr::newPtrLocked(childInode.release());
  for (auto& promise : promises) {
    promise.setValue(inodePtr);
  }
}

Future<unique_ptr<InodeBase>> TreeInode::startLoadingInodeNoThrow(
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
    return makeFuture<unique_ptr<InodeBase>>(
        folly::exception_wrapper{std::current_exception(), ex});
  }
}

Future<unique_ptr<InodeBase>> TreeInode::startLoadingInode(
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
    return make_unique<FileInode>(number, inodePtrFromThis(), name, entry);
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
      self = inodePtrFromThis(),
      childName = PathComponent{name},
      entry,
      number
    ](std::unique_ptr<Tree> tree)->unique_ptr<InodeBase> {
      return make_unique<TreeInode>(
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
  return make_unique<TreeInode>(
      number, inodePtrFromThis(), name, entry, std::move(overlayDir.value()));
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
    inode = FileInodePtr::makeNew(
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
    newChild = TreeInodePtr::makeNew(
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
  // Acquire the rename lock since we need to update our child's location
  auto renameLock = getMount()->acquireRenameLock();

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

  std::unique_ptr<InodeBase> deletedInode;
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
    //
    // FIXME: If our child is not loaded, we need to tell the InodeMap so it
    // can update its state if the child is present in the unloadedInodes_ map.
    if (ent->inode) {
      deletedInode = ent->inode->markUnlinked(this, name, renameLock);
    }

    // And actually remove it
    contents.entries.erase(entIter);
    overlay->saveOverlayDir(myname, &contents);
  });

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  deletedInode.reset();
  return folly::Unit{};
}

folly::Future<folly::Unit> TreeInode::rmdir(PathComponentPiece name) {
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
  // Acquire the rename lock since we need to update our child's location
  auto renameLock = getMount()->acquireRenameLock();

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
  std::unique_ptr<InodeBase> deletedInode;
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
        // Just update to point to the current child, if it is still a tree
        auto* currentChildTree = dynamic_cast<TreeInode*>(ent->inode);
        if (!currentChildTree) {
          throw InodeError(ENOTDIR, inodePtrFromThis(), name);
        }
        child = TreeInodePtr::newPtrLocked(currentChildTree);
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
    // FIXME: If our child is not loaded, we need to tell the InodeMap so it
    // can update its state if the child is present in the unloadedInodes_ map.
    if (ent->inode) {
      deletedInode = ent->inode->markUnlinked(this, name, renameLock);
    }

    // And actually remove it
    contents->entries.erase(entIter);
    overlay->saveOverlayDir(getPathBuggy(), &*contents);
    overlay->removeOverlayDir(targetName);
  }
  deletedInode.reset();

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return folly::Unit{};
}

/**
 * A helper class that stores all locks required to perform a rename.
 *
 * This class helps acquire the locks in the correct order.
 */
struct TreeInode::TreeRenameLocks {
  TreeRenameLocks() {}

  void acquireLocks(
      TreeInode* srcTree,
      TreeInode* destTree,
      PathComponentPiece destName);

  void reset() {
    *this = TreeRenameLocks();
  }

  const RenameLock& renameLock() const {
    return renameLock_;
  }

  Dir* srcContents() {
    return srcContents_;
  }

  Dir* destContents() {
    return destContents_;
  }

  const PathMap<std::unique_ptr<Entry>>::iterator& destChildIter() const {
    return destChildIter_;
  }
  InodeBase* destChild() const {
    DCHECK(destChildExists());
    return destChildIter_->second->inode;
  }

  bool destChildExists() const {
    return destChildIter_ != destContents_->entries.end();
  }
  bool destChildIsDirectory() const {
    DCHECK(destChildExists());
    return mode_to_dtype(destChildIter_->second->mode) == dtype_t::Dir;
  }
  bool destChildIsEmpty() const {
    DCHECK_NOTNULL(destChildContents_);
    return destChildContents_->entries.empty();
  }

 private:
  void lockDestChild(PathComponentPiece destName);

  /**
   * The mountpoint-wide rename lock.
   */
  RenameLock renameLock_;

  /**
   * Locks for the contents of the source and destination directories.
   * If the source and destination directories are the same, only
   * srcContentsLock_ is set.  However, srcContents_ and destContents_ above are
   * always both set, so that destContents_ can be used regardless of wether
   * the source and destination are both the same directory or not.
   */
  folly::Synchronized<Dir>::LockedPtr srcContentsLock_;
  folly::Synchronized<Dir>::LockedPtr destContentsLock_;
  folly::Synchronized<Dir>::LockedPtr destChildContentsLock_;

  /**
   * Pointers to the source and destination directory contents.
   *
   * These may both point to the same contents when the source and destination
   * directory are the same.
   */
  Dir* srcContents_{nullptr};
  Dir* destContents_{nullptr};
  Dir* destChildContents_{nullptr};

  /**
   * An iterator pointing to the destination child entry in
   * destContents_->entries.
   * This may point to destContents_->entries.end() if the destination child
   * does not exist.
   */
  PathMap<std::unique_ptr<Entry>>::iterator destChildIter_;
};

Future<Unit> TreeInode::rename(
    PathComponentPiece name,
    TreeInodePtr destParent,
    PathComponentPiece destName) {
  materializeDirAndParents();
  if (destParent.get() != this) {
    destParent->materializeDirAndParents();
  }

  bool needSrc = false;
  bool needDest = false;
  {
    // Acquire the locks required to do the rename
    TreeRenameLocks locks;
    locks.acquireLocks(this, destParent.get(), destName);

    // Look up the source entry.  The destination entry info was already
    // loaded by TreeRenameLocks::acquireLocks().
    auto srcIter = locks.srcContents()->entries.find(name);
    if (srcIter == locks.srcContents()->entries.end()) {
      // The source path does not exist.  Fail the rename.
      return makeFuture<Unit>(InodeError(ENOENT, inodePtrFromThis(), name));
    }
    Entry* srcEntry = srcIter->second.get();

    // Perform as much input validation as possible now, before starting inode
    // loads that might be necessary.

    // Validate invalid file/directory replacement
    if (mode_to_dtype(srcEntry->mode) == dtype_t::Dir) {
      // The source is a directory.
      // The destination must not exist, or must be an empty directory,
      // or the exact same directory.
      if (locks.destChildExists()) {
        if (!locks.destChildIsDirectory()) {
          VLOG(4) << "attempted to rename directory " << getLogPath() << "/"
                  << name << " over file " << destParent->getLogPath() << "/"
                  << destName;
          return makeFuture<Unit>(InodeError(ENOTDIR, destParent, destName));
        } else if (
            locks.destChild() != srcEntry->inode && !locks.destChildIsEmpty()) {
          VLOG(4) << "attempted to rename directory " << getLogPath() << "/"
                  << name << " over non-empty directory "
                  << destParent->getLogPath() << "/" << destName;
          return makeFuture<Unit>(InodeError(ENOTEMPTY, destParent, destName));
        }
      }
    } else {
      // The source is not a directory.
      // The destination must not exist, or must not be a directory.
      if (locks.destChildExists() && locks.destChildIsDirectory()) {
        VLOG(4) << "attempted to rename file " << getLogPath() << "/" << name
                << " over directory " << destParent->getLogPath() << "/"
                << destName;
        return makeFuture<Unit>(InodeError(EISDIR, destParent, destName));
      }
    }

    // Make sure the destination directory is not unlinked.
    if (destParent->isUnlinked()) {
      VLOG(4) << "attempted to rename file " << getLogPath() << "/" << name
              << " into deleted directory " << destParent->getLogPath()
              << " ( as " << destName << ")";
      return makeFuture<Unit>(InodeError(ENOENT, destParent));
    }

    // Check to see if we need to load the source or destination inodes
    needSrc = !srcEntry->inode;
    needDest = locks.destChildExists() && !locks.destChild();

    // If we don't have to load anything now, we can immediately perform the
    // rename.
    if (!needSrc && !needDest) {
      return doRename(std::move(locks), name, srcIter, destParent, destName);
    }

    // If we are still here we have to load either the source or destination,
    // or both.  Release the locks before we try loading them.
    //
    // (We could refactor getOrLoadChild() a little bit so that we could start
    // the loads with the locks still held, rather than releasing them just for
    // getOrLoadChild() to re-acquire them temporarily.  This isn't terribly
    // important for now, though.)
  }

  // Once we finish the loads, we have to re-run all the rename() logic.
  // Other renames or unlinks may have occurred in the meantime, so all of the
  // validation above has to be redone.
  auto onLoadFinished = [
    self = inodePtrFromThis(),
    nameCopy = name.copy(),
    destParent,
    destNameCopy = destName.copy()
  ]() {
    return self->rename(nameCopy, destParent, destNameCopy);
  };

  if (needSrc && needDest) {
    auto srcFuture = getOrLoadChild(name);
    auto destFuture = destParent->getOrLoadChild(destName);
    return folly::collect(srcFuture, destFuture).then(onLoadFinished);
  } else if (needSrc) {
    return getOrLoadChild(name).then(onLoadFinished);
  } else {
    CHECK(needDest);
    return destParent->getOrLoadChild(destName).then(onLoadFinished);
  }
}

namespace {
bool isAncestor(const RenameLock& renameLock, TreeInode* a, TreeInode* b) {
  auto parent = b->getParent(renameLock);
  while (parent) {
    if (parent.get() == a) {
      return true;
    }
    parent = parent->getParent(renameLock);
  }
  return false;
}
}

Future<Unit> TreeInode::doRename(
    TreeRenameLocks&& locks,
    PathComponentPiece srcName,
    PathMap<std::unique_ptr<Entry>>::iterator srcIter,
    TreeInodePtr destParent,
    PathComponentPiece destName) {
  Entry* srcEntry = srcIter->second.get();

  // If the source and destination refer to exactly the same file,
  // then just succeed immediately.  Nothing needs to be done in this case.
  if (locks.destChildExists() && srcEntry->inode == locks.destChild()) {
    return folly::Unit{};
  }

  // If we are doing a directory rename, sanity check that the destination
  // directory is not a child of the source directory.  The Linux kernel
  // generally should avoid invoking FUSE APIs with an invalid rename like
  // this, but we want to check in case rename() gets invoked via some other
  // non-FUSE mechanism.
  //
  // We don't have to worry about the source being a child of the destination
  // directory.  That will have already been caught by the earlier check that
  // ensures the destination directory is non-empty.
  if (mode_to_dtype(srcEntry->mode) == dtype_t::Dir) {
    // Our caller has already verified that the source is also a
    // directory here.
    auto* srcTreeInode =
        boost::polymorphic_downcast<TreeInode*>(srcEntry->inode);
    if (srcTreeInode == destParent.get() ||
        isAncestor(locks.renameLock(), srcTreeInode, destParent.get())) {
      return makeFuture<Unit>(InodeError(EINVAL, destParent, destName));
    }
  }

  // If we haven't actually materialized it yet, the rename() call will
  // fail.  So don't try that.
  if (srcEntry->materialized) {
    auto contentDir = getOverlay()->getContentDir();
    auto absoluteSourcePath = contentDir + getPathBuggy() + srcName;
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
  std::unique_ptr<InodeBase> deletedInode;
  auto* childInode = srcEntry->inode;
  if (locks.destChildExists()) {
    deletedInode = locks.destChild()->markUnlinked(
        destParent.get(), destName, locks.renameLock());

    // Replace the destination contents entry with the source data
    locks.destChildIter()->second = std::move(srcIter->second);
  } else {
    auto ret = locks.destContents()->entries.emplace(
        destName, std::move(srcIter->second));
    CHECK(ret.second);

    // If the source and destination directory are the same, then inserting the
    // destination entry may have invalidated our source entry iterator, so we
    // have to look it up again.
    if (destParent.get() == this) {
      srcIter = locks.srcContents()->entries.find(srcName);
    }
  }

  // Inform the child inode that it has been moved
  childInode->updateLocation(destParent, destName, locks.renameLock());

  // Now remove the source information
  locks.srcContents()->entries.erase(srcIter);

  // Save the overlay data
  const auto& overlay = getOverlay();
  overlay->saveOverlayDir(getPathBuggy(), locks.srcContents());
  if (destParent.get() != this) {
    // We have already verified that destParent is not unlinked, and we are
    // holding the rename lock which prevents it from being renamed or unlinked
    // while we are operating, so getPath() must have a value here.
    overlay->saveOverlayDir(
        destParent->getPath().value(), locks.destContents());
  }

  // Release the rename locks before we destroy the deleted destination child
  // inode (if it exists).
  locks.reset();
  deletedInode.reset();
  return folly::Unit{};
}

/**
 * Acquire the locks necessary for a rename operation.
 *
 * We acquire multiple locks here:
 *   A) Mountpoint rename lock
 *   B) Source directory contents_ lock
 *   C) Destination directory contents_ lock
 *   E) Destination child contents_ (assuming the destination name
 *      refers to an existing directory).
 *
 * This function ensures the locks are held with the proper ordering.
 * Since we hold the rename lock first, we can acquire multiple TreeInode
 * contents_ locks at once, but we must still ensure that we acquire locks on
 * ancestor TreeInode's before any of their descendants.
 */
void TreeInode::TreeRenameLocks::acquireLocks(
    TreeInode* srcTree,
    TreeInode* destTree,
    PathComponentPiece destName) {
  // First grab the mountpoint-wide rename lock.
  renameLock_ = srcTree->getMount()->acquireRenameLock();

  if (srcTree == destTree) {
    // If the source and destination directories are the same,
    // then there is really only one parent directory to lock.
    srcContentsLock_ = srcTree->contents_.wlock();
    srcContents_ = &*srcContentsLock_;
    destContents_ = &*srcContentsLock_;
    // Look up the destination child entry, and lock it if is is a directory
    lockDestChild(destName);
  } else if (isAncestor(renameLock_, srcTree, destTree)) {
    // If srcTree is an ancestor of destTree, we must acquire the lock on
    // srcTree first.
    srcContentsLock_ = srcTree->contents_.wlock();
    srcContents_ = &*srcContentsLock_;
    destContentsLock_ = destTree->contents_.wlock();
    destContents_ = &*destContentsLock_;
    lockDestChild(destName);
  } else {
    // In all other cases, lock destTree and destChild before srcTree,
    // as long as we verify that destChild and srcTree are not the same.
    //
    // It is not possible for srcTree to be an ancestor of destChild,
    // since we have confirmed that srcTree is not destTree nor an ancestor of
    // destTree.
    destContentsLock_ = destTree->contents_.wlock();
    destContents_ = &*destContentsLock_;
    lockDestChild(destName);

    // While srcTree cannot be an ancestor of destChild, it might be the
    // same inode.  Don't try to lock the same TreeInode twice in this case.
    //
    // The rename will be failed later since this must be an error, but for now
    // we keep going and let the exact error be determined later.
    // This will either be ENOENT (src entry doesn't exist) or ENOTEMPTY
    // (destChild is not empty since the src entry exists).
    if (destChildExists() && destChild() == srcTree) {
      CHECK_NOTNULL(destChildContents_);
      srcContents_ = destChildContents_;
    } else {
      srcContentsLock_ = srcTree->contents_.wlock();
      srcContents_ = &*srcContentsLock_;
    }
  }
}

void TreeInode::TreeRenameLocks::lockDestChild(PathComponentPiece destName) {
  // Look up the destination child entry
  destChildIter_ = destContents_->entries.find(destName);
  if (destChildExists() && destChildIsDirectory() && destChild() != nullptr) {
    auto* childTree = boost::polymorphic_downcast<TreeInode*>(destChild());
    destChildContentsLock_ = childTree->contents_.wlock();
    destChildContents_ = &*destChildContentsLock_;
  }
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

namespace {
folly::Future<folly::Unit> recursivelyLoadMaterializedChildren(
    const InodePtr& child) {
  // If this child is a directory, call loadMaterializedChildren() on it.
  TreeInodePtr treeChild = child.asTreePtrOrNull();
  if (treeChild) {
    return treeChild->loadMaterializedChildren();
  }
  return folly::makeFuture();
}
}

folly::Future<folly::Unit> TreeInode::loadMaterializedChildren() {
  struct LoadInfo {
    LoadInfo(
        Future<unique_ptr<InodeBase>>&& f,
        PathComponentPiece n,
        fuse_ino_t num)
        : future(std::move(f)), name(n), number(num) {}

    Future<unique_ptr<InodeBase>> future;
    PathComponent name;
    fuse_ino_t number;
  };
  std::vector<LoadInfo> pendingLoads;
  std::vector<Future<InodePtr>> inodeFutures;

  {
    auto contents = contents_.wlock();
    if (!contents->materialized) {
      return folly::makeFuture();
    }

    for (auto& entry : contents->entries) {
      const auto& name = entry.first;
      const auto& ent = entry.second;
      if (!ent->materialized) {
        continue;
      }

      if (ent->inode) {
        // We generally don't expect any inodes to be loaded already
        LOG(WARNING)
            << "found already-loaded inode for materialized child "
            << ent->inode->getLogPath()
            << " when performing initial loading of materialized inodes";
        continue;
      }

      folly::Promise<InodePtr> promise;
      inodeFutures.emplace_back(promise.getFuture());
      fuse_ino_t childNumber;
      if (getInodeMap()->shouldLoadChild(
              this, name, std::move(promise), &childNumber)) {
        // The inode is not already being loaded.  We have to start loading it
        // now.
        auto loadFuture =
            startLoadingInodeNoThrow(ent.get(), name, childNumber);
        pendingLoads.emplace_back(std::move(loadFuture), name, childNumber);
      }
    }
  }

  // Hook up the pending load futures to properly complete the loading process
  // then the futures are ready.  We can only do this after releasing the
  // contents_ lock.
  for (auto& load : pendingLoads) {
    registerInodeLoadComplete(load.future, load.name, load.number);
  }

  // Now add callbacks to the Inode futures so that we recurse into
  // children directories when each child inode becomes ready.
  std::vector<Future<folly::Unit>> results;
  for (auto& future : inodeFutures) {
    results.emplace_back(future.then(recursivelyLoadMaterializedChildren));
  }

  return folly::collectAll(results).unit();
}

void TreeInode::unloadChildrenNow() {
  std::vector<TreeInodePtr> treeChildren;
  std::vector<InodeBase*> toDelete;
  auto* inodeMap = getInodeMap();
  {
    auto contents = contents_.wlock();
    auto inodeMapLock = inodeMap->lockForUnload();

    for (auto& entry : contents->entries) {
      if (!entry.second->inode) {
        continue;
      }

      auto* asTree = dynamic_cast<TreeInode*>(entry.second->inode);
      if (asTree) {
        treeChildren.push_back(TreeInodePtr::newPtrLocked(asTree));
      } else {
        if (entry.second->inode->isPtrAcquireCountZero()) {
          // Unload the inode
          inodeMap->unloadInode(
              entry.second->inode, this, entry.first, false, inodeMapLock);
          // Record that we should now delete this inode after releasing
          // the locks.
          toDelete.push_back(entry.second->inode);
          entry.second->inode = nullptr;
        }
      }
    }
  }

  for (auto* child : toDelete) {
    delete child;
  }
  for (auto& child : treeChildren) {
    child->unloadChildrenNow();
  }

  // Note: during mount point shutdown, returning from this function and
  // destroying the treeChildren map will decrement the reference count on
  // all of our children trees, which may result in them being destroyed.
}
}
}
