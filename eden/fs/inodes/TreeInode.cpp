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
#include <folly/FileUtil.h>
#include <folly/futures/Future.h>
#include <vector>
#include "eden/fs/inodes/CheckoutAction.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileData.h"
#include "eden/fs/inodes/FileHandle.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInodeDirHandle.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/Channel.h"
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
using std::vector;

namespace facebook {
namespace eden {

/**
 * A helper class to track info about inode loads that we started while holding
 * the contents_ lock.
 *
 * Once we release the contents_ lock we need to call
 * registerInodeLoadComplete() for each load we started.  This structure
 * exists to remember the arguments for each call that we need to make.
 */
class TreeInode::IncompleteInodeLoad {
 public:
  IncompleteInodeLoad(
      TreeInode* inode,
      Future<unique_ptr<InodeBase>>&& future,
      PathComponentPiece name,
      fuse_ino_t number)
      : treeInode_{inode},
        number_{number},
        name_{name},
        future_{std::move(future)} {}

  IncompleteInodeLoad(IncompleteInodeLoad&&) = default;
  IncompleteInodeLoad& operator=(IncompleteInodeLoad&&) = default;

  ~IncompleteInodeLoad() {
    // Ensure that we always call registerInodeLoadComplete().
    //
    // Normally the caller should always explicitly call finish() after they
    // release the TreeInode's contents_ lock.  However if an exception occurs
    // this might not happen, so we call it ourselves.  We want to make sure
    // this happens even on exception code paths, since the InodeMap will
    // otherwise never be notified about the success or failure of this load
    // attempt, and requests for this inode would just be stuck forever.
    if (treeInode_) {
      LOG(WARNING) << "IncompleteInodeLoad destroyed without explicitly "
                   << "calling finish()";
      finish();
    }
  }

  void finish() {
    // Call treeInode_.release() here before registerInodeLoadComplete() to
    // reset treeInode_ to null.  Setting it to null makes it clear to the
    // destructor that finish() does not need to be called again.
    treeInode_.release()->registerInodeLoadComplete(future_, name_, number_);
  }

 private:
  struct NoopDeleter {
    void operator()(TreeInode*) const {}
  };

  // We store the TreeInode as a unique_ptr just to make sure it gets reset
  // to null in any IncompleteInodeLoad objects that are moved-away from.
  // We don't actually own the TreeInode and we don't destroy it.
  std::unique_ptr<TreeInode, NoopDeleter> treeInode_;
  fuse_ino_t number_;
  PathComponent name_;
  Future<unique_ptr<InodeBase>> future_;
};

TreeInode::TreeInode(
    fuse_ino_t ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    std::unique_ptr<Tree>&& tree)
    : TreeInode(ino, parent, name, buildDirFromTree(tree.get())) {}

TreeInode::TreeInode(
    fuse_ino_t ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    Dir&& dir)
    : InodeBase(ino, parent, name), contents_(std::move(dir)) {
  DCHECK_NE(ino, FUSE_ROOT_ID);
}

TreeInode::TreeInode(EdenMount* mount, std::unique_ptr<Tree>&& tree)
    : TreeInode(mount, buildDirFromTree(tree.get())) {}

TreeInode::TreeInode(EdenMount* mount, Dir&& dir)
    : InodeBase(mount), contents_(std::move(dir)) {}

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
    bool startLoad;
    if (entryPtr->hasInodeNumber()) {
      childNumber = entryPtr->getInodeNumber();
      startLoad = getInodeMap()->shouldLoadChild(
          this, name, childNumber, std::move(promise));
    } else {
      childNumber =
          getInodeMap()->newChildLoadStarted(this, name, std::move(promise));
      // Immediately record the newly allocated inode number
      entryPtr->setInodeNumber(childNumber);
      startLoad = true;
    }
    if (startLoad) {
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

  if (ent->hasInodeNumber()) {
    return ent->getInodeNumber();
  }

  auto inodeNumber = getInodeMap()->allocateInodeNumber();
  ent->setInodeNumber(inodeNumber);
  return inodeNumber;
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
    return make_unique<FileInode>(
        number,
        inodePtrFromThis(),
        name,
        entry->mode,
        entry->getOptionalHash());
  }

  // TODO:
  // - Always load the Tree if this entry has one.  This is needed so we can
  //   compute diffs from the current commit state.  This will simplify
  //   Dirstate computation.
  // - The ObjectStore APIs should be updated to return a Future when loading
  //   the Tree, since this can potentially be a costly operation.
  // - We can potentially start loading the overlay data in parallel with
  //   loading the Tree.

  if (!entry->isMaterialized()) {
    return getStore()->getTreeFuture(entry->getHash()).then([
      self = inodePtrFromThis(),
      childName = PathComponent{name},
      number
    ](std::unique_ptr<Tree> tree)->unique_ptr<InodeBase> {
      return make_unique<TreeInode>(number, self, childName, std::move(tree));
    });
  }

  // No corresponding TreeEntry, this exists only in the overlay.
  CHECK_EQ(number, entry->getInodeNumber());
  auto overlayDir = getOverlay()->loadOverlayDir(number);
  if (!overlayDir) {
    auto bug = EDEN_BUG() << "missing overlay for " << getLogPath() << " / "
                          << name;
    return folly::makeFuture<unique_ptr<InodeBase>>(bug.toException());
  }
  return make_unique<TreeInode>(
      number, inodePtrFromThis(), name, std::move(overlayDir.value()));
}

folly::Future<std::shared_ptr<fusell::DirHandle>> TreeInode::opendir(
    const struct fuse_file_info&) {
  return std::make_shared<TreeInodeDirHandle>(inodePtrFromThis());
}

void TreeInode::materialize(const RenameLock* renameLock) {
  // If we don't have the rename lock yet, do a quick check first
  // to avoid acquiring it if we don't actually need to change anything.
  if (!renameLock) {
    auto contents = contents_.rlock();
    if (contents->materialized) {
      return;
    }
  }

  {
    // Acquire the rename lock now, if it wasn't passed in
    //
    // Only performing materialization state changes with the RenameLock held
    // makes reasoning about update ordering a bit simpler.  This guarantees
    // that materialization and dematerialization operations cannot be
    // interleaved.  We don't want it to be possible for a
    // materialization/dematerialization to interleave the order in which they
    // update the local overlay data and our parent directory's overlay data,
    // possibly resulting in an inconsistent state where the parent thinks we
    // are materialized but we don't think we are.
    RenameLock renameLock2;
    if (!renameLock) {
      renameLock2 = getMount()->acquireRenameLock();
      renameLock = &renameLock2;
    }

    // Write out our data in the overlay before we update our parent.  If we
    // crash partway through it's better if our parent does not say that we are
    // materialized yet even if we actually do have overlay data present,
    // rather than to have our parent indicate that we are materialized but we
    // don't have overlay data present.
    //
    // In the former case, our overlay data should still be identical to the
    // hash mentioned in the parent, so that's fine and we'll still be able to
    // load data correctly the next time we restart.  However, if our parent
    // says we are materialized but we don't actually have overlay data present
    // we won't have any state indicating which source control hash our
    // contents are from.
    {
      auto contents = contents_.wlock();
      // Double check that we still need to be materialized
      if (contents->materialized) {
        return;
      }
      contents->materialized = true;
      getOverlay()->saveOverlayDir(this->getNodeId(), &*contents);
    }

    // Mark ourself materialized in our parent directory (if we have one)
    auto loc = getLocationInfo(*renameLock);
    if (loc.parent && !loc.unlinked) {
      loc.parent->childMaterialized(*renameLock, loc.name, getNodeId());
    }
  }
}

/* If we don't yet have an overlay entry for this portion of the tree,
 * populate it from the Tree.  In order to materialize a dir we have
 * to also materialize its parents. */
void TreeInode::childMaterialized(
    const RenameLock& renameLock,
    PathComponentPiece childName,
    fuse_ino_t childNodeId) {
  {
    auto contents = contents_.wlock();
    auto iter = contents->entries.find(childName);
    if (iter == contents->entries.end()) {
      // This should never happen.
      // We should only get called with legitimate children names.
      EDEN_BUG() << "error attempting to materialize " << childName << " in "
                 << getLogPath() << ": entry not present";
    }

    auto* childEntry = iter->second.get();
    if (contents->materialized && childEntry->isMaterialized()) {
      // Nothing to do
      return;
    }

    childEntry->setMaterialized(childNodeId);
    contents->materialized = true;
    getOverlay()->saveOverlayDir(this->getNodeId(), &*contents);
  }

  // If we have a parent directory, ask our parent to materialize itself
  // and mark us materialized when it does so.
  auto location = getLocationInfo(renameLock);
  if (location.parent && !location.unlinked) {
    location.parent->childMaterialized(renameLock, location.name, getNodeId());
  }
}

void TreeInode::childDematerialized(
    const RenameLock& renameLock,
    PathComponentPiece childName,
    Hash childScmHash) {
  {
    auto contents = contents_.wlock();
    auto iter = contents->entries.find(childName);
    if (iter == contents->entries.end()) {
      // This should never happen.
      // We should only get called with legitimate children names.
      EDEN_BUG() << "error attempting to dematerialize " << childName << " in "
                 << getLogPath() << ": entry not present";
    }

    auto* childEntry = iter->second.get();
    if (!childEntry->isMaterialized() &&
        childEntry->getHash() == childScmHash) {
      // Nothing to do.  Our child's state and our own are both unchanged.
      return;
    }

    // Mark the child dematerialized.
    childEntry->setDematerialized(childScmHash);

    // Mark us materialized!
    //
    // Even though our child is dematerialized, we always materialize ourself
    // so we make sure we record the correct source control hash for our child.
    // Currently dematerialization only happens on the checkout() flow.  Once
    // checkout finishes processing all of the children it will call
    // saveOverlayPostCheckout() on this directory, and here we will check to
    // see if we can dematerialize ourself.
    contents->materialized = true;
    getOverlay()->saveOverlayDir(this->getNodeId(), &*contents);
  }

  // We are materialized now.
  // If we have a parent directory, ask our parent to materialize itself
  // and mark us materialized when it does so.
  auto location = getLocationInfo(renameLock);
  if (location.parent && !location.unlinked) {
    location.parent->childMaterialized(renameLock, location.name, getNodeId());
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
    Entry entry{treeEntry.getMode(), treeEntry.getHash()};
    dir.entries.emplace(
        treeEntry.getName(), std::make_unique<Entry>(std::move(entry)));
  }
  return dir;
}

folly::Future<TreeInode::CreateResult>
TreeInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Compute the effective name of the node they want to create.
  RelativePath targetName;
  std::shared_ptr<FileHandle> handle;
  FileInodePtr inode;

  materialize();

  // We need to scope the write lock as the getattr call below implicitly
  // wants to acquire a read lock.
  {
    // Acquire our contents lock
    auto contents = contents_.wlock();

    auto myPath = getPath();
    // Make sure this directory has not been unlinked.
    // We have to check this after acquiring the contents_ lock; otherwise
    // we could race with rmdir() or rename() calls affecting us.
    if (!myPath.hasValue()) {
      return makeFuture<CreateResult>(InodeError(ENOENT, inodePtrFromThis()));
    }
    // Compute the target path, so we can record it in the journal below.
    targetName = myPath.value() + name;

    // Generate an inode number for this new entry.
    auto* inodeMap = this->getInodeMap();
    auto childNumber = inodeMap->allocateInodeNumber();

    // Since we will move this file into the underlying file data, we
    // take special care to ensure that it is opened read-write
    auto filePath = getOverlay()->getFilePath(childNumber);
    folly::File file(
        filePath.c_str(),
        O_RDWR | O_CREAT | (flags & ~(O_RDONLY | O_WRONLY)),
        0600);

    // The mode passed in by the caller may not have the file type bits set.
    // Ensure that we mark this as a regular file.
    mode = S_IFREG | (07777 & mode);

    // Record the new entry
    auto& entry = contents->entries[name];
    entry = std::make_unique<Entry>(mode, childNumber);

    // build a corresponding FileInode
    inode = FileInodePtr::makeNew(
        childNumber, this->inodePtrFromThis(), name, mode, std::move(file));
    entry->inode = inode.get();
    inodeMap->inodeCreated(inode);

    // The kernel wants an open operation to return the inode,
    // the file handle and some attribute information.
    // Let's open a file handle now.
    handle = inode->finishCreate();

    this->getOverlay()->saveOverlayDir(getNodeId(), &*contents);
  }

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

FileInodePtr TreeInode::symlink(
    PathComponentPiece name,
    folly::StringPiece symlinkTarget) {
  // Compute the effective name of the node they want to create.
  RelativePath targetName;
  std::shared_ptr<FileHandle> handle;
  FileInodePtr inode;

  materialize();

  // We need to scope the write lock as the getattr call below implicitly
  // wants to acquire a read lock.
  {
    // Acquire our contents lock
    auto contents = contents_.wlock();

    auto myPath = getPath();
    // Make sure this directory has not been unlinked.
    // We have to check this after acquiring the contents_ lock; otherwise
    // we could race with rmdir() or rename() calls affecting us.
    if (!myPath.hasValue()) {
      throw InodeError(ENOENT, inodePtrFromThis());
    }
    // Compute the target path, so we can record it in the journal below.
    targetName = myPath.value() + name;

    auto entIter = contents->entries.find(name);
    if (entIter != contents->entries.end()) {
      throw InodeError(EEXIST, this->inodePtrFromThis(), name);
    }

    // Generate an inode number for this new entry.
    auto* inodeMap = this->getInodeMap();
    auto childNumber = inodeMap->allocateInodeNumber();

    auto filePath = getOverlay()->getFilePath(childNumber);

    folly::File file(filePath.c_str(), O_RDWR | O_CREAT | O_EXCL, 0600);
    SCOPE_FAIL {
      ::unlink(filePath.c_str());
    };
    auto wrote = folly::writeNoInt(
        file.fd(), symlinkTarget.data(), symlinkTarget.size());
    if (wrote == -1) {
      folly::throwSystemError("writeNoInt(", filePath, ") failed");
    }
    if (wrote != symlinkTarget.size()) {
      folly::throwSystemError(
          "writeNoInt(",
          filePath,
          ") wrote only ",
          wrote,
          " of ",
          symlinkTarget.size(),
          " bytes");
    }

    auto entry = std::make_unique<Entry>(S_IFLNK | 0770, childNumber);

    // build a corresponding FileInode
    inode = FileInodePtr::makeNew(
        childNumber,
        this->inodePtrFromThis(),
        name,
        entry->mode,
        std::move(file));
    entry->inode = inode.get();
    inodeMap->inodeCreated(inode);
    contents->entries.emplace(name, std::move(entry));

    this->getOverlay()->saveOverlayDir(getNodeId(), &*contents);
  }

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return inode;
}

TreeInodePtr TreeInode::mkdir(PathComponentPiece name, mode_t mode) {
  RelativePath targetName;
  // Compute the effective name of the node they want to create.
  materialize();

  TreeInodePtr newChild;
  {
    // Acquire our contents lock
    auto contents = contents_.wlock();

    auto myPath = getPath();
    // Make sure this directory has not been unlinked.
    // We have to check this after acquiring the contents_ lock; otherwise
    // we could race with rmdir() or rename() calls affecting us.
    if (!myPath.hasValue()) {
      throw InodeError(ENOENT, inodePtrFromThis());
    }
    // Compute the target path, so we can record it in the journal below.
    targetName = myPath.value() + name;

    auto entIter = contents->entries.find(name);
    if (entIter != contents->entries.end()) {
      throw InodeError(EEXIST, this->inodePtrFromThis(), name);
    }
    auto overlay = this->getOverlay();

    // Allocate an inode number
    auto* inodeMap = this->getInodeMap();
    auto childNumber = inodeMap->allocateInodeNumber();

    // The mode passed in by the caller may not have the file type bits set.
    // Ensure that we mark this as a directory.
    mode = S_IFDIR | (07777 & mode);

    // Store the overlay entry for this dir
    Dir emptyDir;
    emptyDir.materialized = true;
    overlay->saveOverlayDir(childNumber, &emptyDir);

    // Add a new entry to contents_.entries
    auto emplaceResult = contents->entries.emplace(
        name, std::make_unique<Entry>(mode, childNumber));
    CHECK(emplaceResult.second)
        << "directory contents should not have changed since the check above";
    auto& entry = emplaceResult.first->second;

    // Create the TreeInode
    newChild = TreeInodePtr::makeNew(
        childNumber,
        this->inodePtrFromThis(),
        name,
        std::move(emptyDir));
    entry->inode = newChild.get();
    inodeMap->inodeCreated(newChild);

    // Save our updated overlay data
    overlay->saveOverlayDir(getNodeId(), &*contents);
  }

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return newChild;
}

folly::Future<folly::Unit> TreeInode::unlink(PathComponentPiece name) {
  return getOrLoadChild(name).then(
      [ self = inodePtrFromThis(),
        childName = PathComponent{name} ](const InodePtr& child) {
        return self->removeImpl<FileInodePtr>(childName, child, 1);
      });
}

folly::Future<folly::Unit> TreeInode::rmdir(PathComponentPiece name) {
  return getOrLoadChild(name).then(
      [ self = inodePtrFromThis(),
        childName = PathComponent{name} ](const InodePtr& child) {
        return self->removeImpl<TreeInodePtr>(childName, child, 1);
      });
}

template <typename InodePtrType>
folly::Future<folly::Unit> TreeInode::removeImpl(
    PathComponent name,
    InodePtr childBasePtr,
    unsigned int attemptNum) {
  // Acquire the rename lock since we need to update our child's location
  auto renameLock = getMount()->acquireRenameLock();

  // Make sure the child is of the desired type
  auto child = childBasePtr.asSubclassPtrOrNull<InodePtrType>();
  if (!child) {
    return makeFuture<Unit>(
        InodeError(InodePtrType::InodeType::WRONG_TYPE_ERRNO, child));
  }

  // Verify that we can remove the child before we materialize ourself
  checkPreRemove(child);

  materialize(&renameLock);

  // Lock our contents in write mode.
  // We will hold it for the duration of the unlink.
  RelativePath targetName;
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
      // This child was replaced since the remove attempt started.
      if (ent->inode == nullptr) {
        constexpr unsigned int kMaxRemoveRetries = 3;
        if (attemptNum > kMaxRemoveRetries) {
          throw InodeError(
              EIO,
              inodePtrFromThis(),
              name,
              "inode was removed/renamed after remove started");
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
        auto childFuture = getOrLoadChild(name);
        return childFuture.then([
          self = inodePtrFromThis(),
          childName = PathComponent{std::move(name)},
          attemptNum
        ](const InodePtr& loadedChild) {
          return self->removeImpl<InodePtrType>(
              childName, loadedChild, attemptNum + 1);
        });
      } else {
        // Just update to point to the current child, if it is still a tree
        auto* currentChild =
            dynamic_cast<typename InodePtrType::InodeType*>(ent->inode);
        if (!currentChild) {
          throw InodeError(
              InodePtrType::InodeType::WRONG_TYPE_ERRNO,
              inodePtrFromThis(),
              name);
        }
        child = InodePtrType::newPtrLocked(currentChild);
      }
    }

    // Get the path to the child, so we can update the journal later.
    auto myPath = getPath();
    if (!myPath.hasValue()) {
      // This shouldn't be possible.  We cannot be unlinked
      // if we still contain a child.
      LOG(FATAL) << "found unlinked but non-empty directory: " << getLogPath()
                 << " still contains " << name;
    }
    targetName = myPath.value() + name;

    // Verify that the child is still in a good state to remove
    checkPreRemove(child);

    // Inform the child it is now unlinked
    deletedInode = child->markUnlinked(this, name, renameLock);

    // Remove it from our entries list
    contents->entries.erase(entIter);

    // Update the on-disk overlay
    auto overlay = this->getOverlay();
    overlay->saveOverlayDir(getNodeId(), &*contents);
  }
  deletedInode.reset();

  getMount()->getJournal().wlock()->addDelta(
      std::make_unique<JournalDelta>(JournalDelta{targetName}));

  return folly::Unit{};
}

void TreeInode::checkPreRemove(const TreeInodePtr& child) {
  // Lock the child contents, and make sure they are empty
  auto childContents = child->contents_.rlock();
  if (!childContents->entries.empty()) {
    throw InodeError(ENOTEMPTY, child);
  }
}

void TreeInode::checkPreRemove(const FileInodePtr& /* child */) {
  // Nothing to do
}

/**
 * A helper class that stores all locks required to perform a rename.
 *
 * This class helps acquire the locks in the correct order.
 */
class TreeInode::TreeRenameLocks {
 public:
  TreeRenameLocks() {}

  void acquireLocks(
      RenameLock&& renameLock,
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
  bool needSrc = false;
  bool needDest = false;
  {
    auto renameLock = getMount()->acquireRenameLock();
    materialize(&renameLock);
    if (destParent.get() != this) {
      destParent->materialize(&renameLock);
    }

    // Acquire the locks required to do the rename
    TreeRenameLocks locks;
    locks.acquireLocks(std::move(renameLock), this, destParent.get(), destName);

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
  overlay->saveOverlayDir(getNodeId(), locks.srcContents());
  if (destParent.get() != this) {
    // We have already verified that destParent is not unlinked, and we are
    // holding the rename lock which prevents it from being renamed or unlinked
    // while we are operating, so getPath() must have a value here.
    overlay->saveOverlayDir(destParent->getNodeId(), locks.destContents());
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
    RenameLock&& renameLock,
    TreeInode* srcTree,
    TreeInode* destTree,
    PathComponentPiece destName) {
  // Store the mountpoint-wide rename lock.
  renameLock_ = std::move(renameLock);

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

Future<Unit> TreeInode::checkout(
    CheckoutContext* ctx,
    std::unique_ptr<Tree> fromTree,
    std::unique_ptr<Tree> toTree) {
  VLOG(4) << "checkout: starting update of " << getLogPath() << ": "
          << fromTree->getHash() << " --> " << toTree->getHash();
  vector<unique_ptr<CheckoutAction>> actions;
  vector<IncompleteInodeLoad> pendingLoads;

  computeCheckoutActions(
      ctx, fromTree.get(), toTree.get(), &actions, &pendingLoads);

  // Wire up the callbacks for any pending inode loads we started
  for (auto& load : pendingLoads) {
    load.finish();
  }

  // Now start all of the checkout actions
  vector<Future<Unit>> actionFutures;
  for (const auto& action : actions) {
    actionFutures.emplace_back(action->run(ctx, getStore()));
  }
  // Wait for all of the actions, and record any errors.
  return folly::collectAll(actionFutures).then([
    ctx,
    self = inodePtrFromThis(),
    toTree = std::move(toTree),
    actions = std::move(actions)
  ](vector<folly::Try<Unit>> actionResults) {
    // Record any errors that occurred
    size_t numErrors = 0;
    for (size_t n = 0; n < actionResults.size(); ++n) {
      auto& result = actionResults[n];
      if (!result.hasException()) {
        continue;
      }
      ++numErrors;
      ctx->addError(self.get(), actions[n]->getEntryName(), result.exception());
    }

    // Update our state in the overlay
    self->saveOverlayPostCheckout(ctx, toTree.get());

    VLOG(4) << "checkout: finished update of " << self->getLogPath() << ": "
            << numErrors << " errors";
  });
}

void TreeInode::computeCheckoutActions(
    CheckoutContext* ctx,
    const Tree* fromTree,
    const Tree* toTree,
    vector<unique_ptr<CheckoutAction>>* actions,
    vector<IncompleteInodeLoad>* pendingLoads) {
  // Grab the contents_ lock for the duration of this function
  auto contents = contents_.wlock();

  // Walk through fromTree and toTree, and call the above helper functions as
  // appropriate.
  //
  // Note that we completely ignore entries in our current contents_ that don't
  // appear in either fromTree or toTree.  These are untracked in both the old
  // and new trees.
  size_t oldIdx = 0;
  size_t newIdx = 0;
  vector<TreeEntry> emptyEntries;
  const auto& oldEntries = fromTree ? fromTree->getTreeEntries() : emptyEntries;
  const auto& newEntries = toTree->getTreeEntries();
  while (true) {
    unique_ptr<CheckoutAction> action;

    if (oldIdx >= oldEntries.size()) {
      if (newIdx >= newEntries.size()) {
        // All Done
        break;
      }

      // This entry is present in the new tree but not the old one.
      action = processCheckoutEntry(
          ctx, *contents, nullptr, &newEntries[newIdx], pendingLoads);
      ++newIdx;
    } else if (newIdx >= newEntries.size()) {
      // This entry is present in the old tree but not the old one.
      action = processCheckoutEntry(
          ctx, *contents, &oldEntries[oldIdx], nullptr, pendingLoads);
      ++oldIdx;
    } else if (oldEntries[oldIdx].getName() < newEntries[newIdx].getName()) {
      action = processCheckoutEntry(
          ctx, *contents, &oldEntries[oldIdx], nullptr, pendingLoads);
      ++oldIdx;
    } else if (oldEntries[oldIdx].getName() > newEntries[newIdx].getName()) {
      action = processCheckoutEntry(
          ctx, *contents, nullptr, &newEntries[newIdx], pendingLoads);
      ++newIdx;
    } else {
      action = processCheckoutEntry(
          ctx,
          *contents,
          &oldEntries[oldIdx],
          &newEntries[newIdx],
          pendingLoads);
      ++oldIdx;
      ++newIdx;
    }

    if (action) {
      actions->push_back(std::move(action));
    }
  }
}

unique_ptr<CheckoutAction> TreeInode::processCheckoutEntry(
    CheckoutContext* ctx,
    Dir& contents,
    const TreeEntry* oldScmEntry,
    const TreeEntry* newScmEntry,
    vector<IncompleteInodeLoad>* pendingLoads) {
  // At most one of oldScmEntry and newScmEntry may be null.
  DCHECK(oldScmEntry || newScmEntry);

  // If we aren't doing a force checkout, we don't need to do anything
  // for entries that are identical between the old and new source control
  // trees.
  //
  // If we are doing a force checkout we need to process unmodified entries to
  // revert them to the desired state if they were modified in the local
  // filesystem.
  if (!ctx->forceUpdate() && oldScmEntry && newScmEntry &&
      oldScmEntry->getMode() == newScmEntry->getMode() &&
      oldScmEntry->getHash() == newScmEntry->getHash()) {
    // TODO: Should we perhaps fall through anyway to report conflicts for
    // locally modified files?
    return nullptr;
  }

  // Look to see if we have a child entry with this name.
  const auto& name =
      oldScmEntry ? oldScmEntry->getName() : newScmEntry->getName();
  auto it = contents.entries.find(name);
  if (it == contents.entries.end()) {
    if (!oldScmEntry) {
      // This is a new entry being added, that did not exist in the old tree
      // and does not currently exist in the filesystem.  Go ahead and add it
      // now.
      if (ctx->shouldApplyChanges()) {
        auto newEntry =
            make_unique<Entry>(newScmEntry->getMode(), newScmEntry->getHash());
        contents.entries.emplace(newScmEntry->getName(), std::move(newEntry));
      }
    } else if (!newScmEntry) {
      // This file exists in the old tree, but is being removed in the new
      // tree.  It has already been removed from the local filesystem, so
      // we are already in the desired state.
      //
      // We can proceed, but we still flag this as a conflict.
      ctx->addConflict(
          ConflictType::MISSING_REMOVED, this, oldScmEntry->getName());
    } else {
      // The file was removed locally, but modified in the new tree.
      ctx->addConflict(
          ConflictType::REMOVED_MODIFIED, this, oldScmEntry->getName());
      if (ctx->forceUpdate()) {
        DCHECK(ctx->shouldApplyChanges());
        auto newEntry =
            make_unique<Entry>(newScmEntry->getMode(), newScmEntry->getHash());
        contents.entries.emplace(newScmEntry->getName(), std::move(newEntry));
      }
    }

    // Nothing else to do when there is no local inode.
    return nullptr;
  }

  auto& entry = it->second;
  if (entry->inode) {
    // If the inode is already loaded, create a CheckoutAction to process it
    auto childPtr = InodePtr::newPtrLocked(entry->inode);
    return make_unique<CheckoutAction>(
        ctx, oldScmEntry, newScmEntry, std::move(childPtr));
  }

  // If this entry has an inode number assigned to it then load the InodeBase
  // object to process it.
  //
  // We have to load the InodeBase object because another thread may already be
  // trying to load it.
  //
  // This also handles materialized inodes--an inode cannot be materialized if
  // it does not have an inode number assigned to it.
  if (entry->hasInodeNumber()) {
    // This child is potentially modified, but is not currently loaded.
    // Start loading it and create a CheckoutAction to process it once it
    // is loaded.
    auto inodeFuture =
        loadChildLocked(contents, name, entry.get(), pendingLoads);
    return make_unique<CheckoutAction>(
        ctx, oldScmEntry, newScmEntry, std::move(inodeFuture));
  }

  // Check for conflicts
  auto conflictType = ConflictType::ERROR;
  if (!oldScmEntry) {
    conflictType = ConflictType::UNTRACKED_ADDED;
  } else if (entry->getHash() != oldScmEntry->getHash()) {
    conflictType = ConflictType::MODIFIED;
  }
  if (conflictType != ConflictType::ERROR) {
    // If this is are a directory we unfortunately have to load the directory
    // and recurse into it just so we can accurately report the list of files
    // with conflicts.
    if (mode_to_dtype(entry->mode) == dtype_t::Dir) {
      auto inodeFuture =
          loadChildLocked(contents, name, entry.get(), pendingLoads);
      return make_unique<CheckoutAction>(
          ctx, oldScmEntry, newScmEntry, std::move(inodeFuture));
    }

    // Report the conflict, and then bail out if we aren't doing a force update
    ctx->addConflict(conflictType, this, name);
    if (!ctx->forceUpdate()) {
      return nullptr;
    }
  }

  // Bail out now if we aren't actually supposed to apply changes.
  if (!ctx->shouldApplyChanges()) {
    return nullptr;
  }

  // Update the entry
  if (!newScmEntry) {
    contents.entries.erase(it);
  } else {
    *entry = Entry{newScmEntry->getMode(), newScmEntry->getHash()};
  }

  // Note that we intentionally don't bother calling
  // fuseChannel->invalidateEntry() here.
  //
  // We always assign an inode number to entries when telling FUSE about
  // directory entries.  Given that this entry does not have an inode number we
  // must not have ever told FUSE about it.

  return nullptr;
}

Future<Unit> TreeInode::checkoutReplaceEntry(
    CheckoutContext* ctx,
    InodePtr inode,
    const TreeEntry& newScmEntry) {
  CHECK(ctx->shouldApplyChanges());
  return checkoutRemoveChild(ctx, newScmEntry.getName(), inode)
      .then([ self = inodePtrFromThis(), newScmEntry ]() {
        auto contents = self->contents_.wlock();
        auto newEntry =
            make_unique<Entry>(newScmEntry.getMode(), newScmEntry.getHash());
        contents->entries.emplace(newScmEntry.getName(), std::move(newEntry));
      });
}

Future<Unit> TreeInode::checkoutRemoveChild(
    CheckoutContext* ctx,
    PathComponentPiece name,
    InodePtr inode) {
  CHECK(ctx->shouldApplyChanges());
  std::unique_ptr<InodeBase> deletedInode;
  auto contents = contents_.wlock();

  // The CheckoutContext should be holding the rename lock, so the entry
  // at this name should still be the specified inode.
  auto it = contents->entries.find(name);
  if (it == contents->entries.end()) {
    auto bug = EDEN_BUG()
        << "entry removed while holding rename lock during checkout: "
        << inode->getLogPath();
    return folly::makeFuture<Unit>(bug.toException());
  }
  if (it->second->inode != inode.get()) {
    auto bug = EDEN_BUG()
        << "entry changed while holding rename lock during checkout: "
        << inode->getLogPath();
    return folly::makeFuture<Unit>(bug.toException());
  }

  auto treeInode = inode.asTreePtrOrNull();
  if (!treeInode) {
    // This is a file, so we can simply unlink it
    deletedInode = inode->markUnlinked(this, name, ctx->renameLock());
    contents->entries.erase(it);

    // Tell FUSE to invalidate it's cache for this entry.
    auto* fuseChannel = getMount()->getFuseChannel();
    if (fuseChannel) {
      fuseChannel->invalidateEntry(getNodeId(), name);
    }

    // We don't save our own overlay data right now:
    // we'll wait to do that until the checkout operation finishes touching all
    // of our children in checkout().
    return makeFuture();
  }

  // We have to recursively unlink everything inside this tree
  // FIXME
  return makeFuture<Unit>(std::runtime_error(
      "TreeInode::checkoutRemoveChild() not implemented for trees"));
}

void TreeInode::saveOverlayPostCheckout(
    CheckoutContext* ctx,
    const Tree* tree) {
  bool materialize;
  bool stateChanged;
  {
    auto contents = contents_.wlock();

    // Check to see if we need to be materialized or not.
    //
    // If we can confirm that we are identical to the source control Tree we do
    // not need to be materialized.
    auto shouldMaterialize = [&]() {
      const auto& scmEntries = tree->getTreeEntries();
      // If we have a different number of entries we must be different from the
      // Tree, and therefore must be materialized.
      if (scmEntries.size() != contents->entries.size()) {
        return true;
      }

      // This code relies on the fact that our contents->entries PathMap sorts
      // paths in the same order as Tree's entry list.
      auto inodeIter = contents->entries.begin();
      auto scmIter = scmEntries.begin();
      for (; scmIter != scmEntries.end(); ++inodeIter, ++scmIter) {
        // If any of our children are materialized, we need to be materialized
        // too to record the fact that we have materialized children.
        //
        // If our children are materialized this means they are likely different
        // from the new source control state.  (This is not a 100% guarantee
        // though, as writes may still be happening concurrently to the checkout
        // operation.)  Even if the child is still identical to its source
        // control state we still want to make sure we are materialized if the
        // child is.
        if (inodeIter->second->isMaterialized()) {
          return true;
        }

        // If if the child is not materialized, it is the same as some source
        // control object.  However, if it isn't the same as the object in our
        // Tree, we have to materialize ourself.
        if (inodeIter->second->getHash() != scmIter->getHash()) {
          return true;
        }
      }

      // If we're still here we are identical to the source control Tree.
      // We can be dematerialized.
      return false;
    };

    materialize = shouldMaterialize();
    stateChanged = (materialize != contents->materialized);
    if (materialize) {
      // If we need to be materialized, write out our state to the overlay.
      // (It's possible our state is unchanged from what's already on disk,
      // but for now we can't detect this, and just always write it out.)
      getOverlay()->saveOverlayDir(getNodeId(), &*contents);
    }
    contents->materialized = materialize;
  }

  // If our state changed, tell our parent.
  //
  // TODO: Currently we end up writing out overlay data for TreeInodes pretty
  // often during the checkout process.  Each time a child entry is processed
  // we will likely end up rewriting data for it's parent TreeInode, and then
  // once all children are processed we do another pass through here in
  // saveOverlayPostCheckout() and possibly write it out again.
  //
  // It would be nicer if we could only save the data for each TreeInode once.
  // The downside of this is that the on-disk overlay state would be
  // potentially inconsistent until the checkout completes.  There may be
  // periods of time where a parent directory says the child is materialized
  // when the child has decided to be dematerialized.  This would cause
  // problems when we tried to load the overlay data later.  If we update the
  // code to be able to handle this somehow then maybe we could avoid doing all
  // of the intermediate updates to the parent as we process each child entry.
  if (stateChanged) {
    auto loc = getLocationInfo(ctx->renameLock());
    if (loc.parent && !loc.unlinked) {
      if (materialize) {
        loc.parent->childMaterialized(ctx->renameLock(), loc.name, getNodeId());
      } else {
        loc.parent->childDematerialized(
            ctx->renameLock(), loc.name, tree->getHash());
      }
    }

    // If we were dematerialized, remove our overlay data only after updating
    // our parent.  This ensures that we always have overlay data on disk when
    // our parent thinks we do.
    if (!materialize) {
      getOverlay()->removeOverlayData(getNodeId());
    }
  }
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

folly::Future<InodePtr> TreeInode::loadChildLocked(
    Dir& /* contents */,
    PathComponentPiece name,
    Entry* entry,
    std::vector<IncompleteInodeLoad>* pendingLoads) {
  DCHECK(!entry->inode);

  bool startLoad;
  fuse_ino_t childNumber;
  folly::Promise<InodePtr> promise;
  auto future = promise.getFuture();
  if (entry->hasInodeNumber()) {
    childNumber = entry->getInodeNumber();
    startLoad = getInodeMap()->shouldLoadChild(
        this, name, childNumber, std::move(promise));
  } else {
    childNumber =
        getInodeMap()->newChildLoadStarted(this, name, std::move(promise));
    // Immediately record the newly allocated inode number
    entry->setInodeNumber(childNumber);
    startLoad = true;
  }

  if (startLoad) {
    auto loadFuture =
        startLoadingInodeNoThrow(entry, name, entry->getInodeNumber());
    pendingLoads->emplace_back(
        this, std::move(loadFuture), name, entry->getInodeNumber());
  }

  return future;
}

folly::Future<folly::Unit> TreeInode::loadMaterializedChildren() {
  std::vector<IncompleteInodeLoad> pendingLoads;
  std::vector<Future<InodePtr>> inodeFutures;

  {
    auto contents = contents_.wlock();
    if (!contents->materialized) {
      return folly::makeFuture();
    }

    for (auto& entry : contents->entries) {
      const auto& name = entry.first;
      const auto& ent = entry.second;
      if (!ent->isMaterialized()) {
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

      auto future = loadChildLocked(*contents, name, ent.get(), &pendingLoads);
      inodeFutures.emplace_back(std::move(future));
    }
  }

  // Hook up the pending load futures to properly complete the loading process
  // then the futures are ready.  We can only do this after releasing the
  // contents_ lock.
  for (auto& load : pendingLoads) {
    load.finish();
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
