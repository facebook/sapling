/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/TreeInode.h"

#include <boost/polymorphic_cast.hpp>
#include <folly/FileUtil.h>
#include <folly/chrono/Conv.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <vector>

#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/RequestData.h"
#include "eden/fs/inodes/CheckoutAction.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DeferredDiffEntry.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreePrefetchLease.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/DiffCallback.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/Synchronized.h"
#include "eden/fs/utils/TimeUtil.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"
#include "eden/fs/utils/XAttr.h"

using folly::ByteRange;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
using std::make_unique;
using std::shared_ptr;
using std::unique_ptr;
using std::vector;

namespace facebook {
namespace eden {

namespace {
static constexpr PathComponentPiece kIgnoreFilename{".gitignore"};
}

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
      InodeNumber number)
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
      XLOG(WARNING) << "IncompleteInodeLoad destroyed without explicitly "
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
  InodeNumber number_;
  PathComponent name_;
  Future<unique_ptr<InodeBase>> future_;
};

TreeInode::TreeInode(
    InodeNumber ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    mode_t initialMode,
    std::shared_ptr<const Tree>&& tree)
    : TreeInode(
          ino,
          parent,
          name,
          initialMode,
          std::nullopt,
          saveDirFromTree(ino, tree.get(), parent->getMount()),
          tree->getHash()) {}

TreeInode::TreeInode(
    InodeNumber ino,
    TreeInodePtr parent,
    PathComponentPiece name,
    mode_t initialMode,
    const std::optional<InodeTimestamps>& initialTimestamps,
    DirContents&& dir,
    std::optional<Hash> treeHash)
    : Base(ino, initialMode, initialTimestamps, parent, name),
      contents_(folly::in_place, std::move(dir), treeHash) {
  DCHECK_NE(ino, kRootNodeId);
}

TreeInode::TreeInode(EdenMount* mount, std::shared_ptr<const Tree>&& tree)
    : TreeInode(
          mount,
          saveDirFromTree(kRootNodeId, tree.get(), mount),
          tree->getHash()) {}

TreeInode::TreeInode(
    EdenMount* mount,
    DirContents&& dir,
    std::optional<Hash> treeHash)
    : Base(mount), contents_(folly::in_place, std::move(dir), treeHash) {}

TreeInode::~TreeInode() {}

folly::Future<Dispatcher::Attr> TreeInode::getattr() {
  return getAttrLocked(contents_.rlock()->entries);
}

Dispatcher::Attr TreeInode::getAttrLocked(const DirContents& contents) {
  Dispatcher::Attr attr(getMount()->initStatData());

  attr.st.st_ino = getNodeId().get();
  getMetadataLocked(contents).applyToStat(attr.st);

  // For directories, nlink is the number of entries including the
  // "." and ".." links.
  attr.st.st_nlink = contents.size() + 2;
  return attr;
}

Future<InodePtr> TreeInode::getOrLoadChild(PathComponentPiece name) {
  TraceBlock block("getOrLoadChild");

  if (name == kDotEdenName && getNodeId() != kRootNodeId) {
    // If they ask for `.eden` in any subdir, return the magical
    // this-dir symlink inode that resolves to the path to the
    // root/.eden path.  We do this outside of the block below
    // because getInode() will call TreeInode::getOrLoadChild()
    // recursively, and it is cleaner to break this logic out
    // separately.
    return getMount()->getInode(".eden/this-dir"_relpath);
  }

  return tryRlockCheckBeforeUpdate<Future<InodePtr>>(
             contents_,
             [&](const auto& contents) -> folly::Optional<Future<InodePtr>> {
               // Check if the child is already loaded and return it if so
               auto iter = contents.entries.find(name);
               if (iter == contents.entries.end()) {
                 XLOG(DBG7) << "attempted to load non-existent entry \"" << name
                            << "\" in " << getLogPath();
                 return folly::make_optional(makeFuture<InodePtr>(
                     InodeError(ENOENT, inodePtrFromThis(), name)));
               }

               // Check to see if the entry is already loaded
               const auto& entry = iter->second;
               if (entry.getInode()) {
                 return makeFuture<InodePtr>(entry.getInodePtr());
               }
               return folly::none;
             },
             [&](auto& contents) {
               auto inodeLoadFuture =
                   Future<unique_ptr<InodeBase>>::makeEmpty();
               auto returnFuture = Future<InodePtr>::makeEmpty();
               InodePtr childInodePtr;
               InodeMap::PromiseVector promises;
               InodeNumber childNumber;

               // The entry is not loaded yet.  Ask the InodeMap about the
               // entry. The InodeMap will tell us if this inode is already in
               // the process of being loaded, or if we need to start loading it
               // now.
               auto iter = contents->entries.find(name);
               auto& entry = iter->second;
               folly::Promise<InodePtr> promise;
               returnFuture = promise.getFuture();
               childNumber = entry.getInodeNumber();
               bool startLoad = getInodeMap()->shouldLoadChild(
                   this, name, childNumber, std::move(promise));
               if (startLoad) {
                 // The inode is not already being loaded.  We have to start
                 // loading it now.
                 auto loadFuture = startLoadingInodeNoThrow(entry, name);
                 if (loadFuture.isReady() && loadFuture.hasValue()) {
                   // If we finished loading the inode immediately, just call
                   // InodeMap::inodeLoadComplete() now, since we still have the
                   // data_ lock.
                   auto childInode = std::move(loadFuture).get();
                   entry.setInode(childInode.get());
                   promises =
                       getInodeMap()->inodeLoadComplete(childInode.get());
                   childInodePtr =
                       InodePtr::takeOwnership(std::move(childInode));
                 } else {
                   inodeLoadFuture = std::move(loadFuture);
                 }
               }
               contents.unlock();
               if (inodeLoadFuture.valid()) {
                 registerInodeLoadComplete(inodeLoadFuture, name, childNumber);
               } else {
                 for (auto& promise : promises) {
                   promise.setValue(childInodePtr);
                 }
               }

               return returnFuture;
             })
      .ensure([b = std::move(block)]() mutable { b.close(); });
}

Future<TreeInodePtr> TreeInode::getOrLoadChildTree(PathComponentPiece name) {
  return getOrLoadChild(name).thenValue([](InodePtr child) {
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
} // namespace

Future<InodePtr> TreeInode::getChildRecursive(RelativePathPiece path) {
  auto pathStr = path.stringPiece();
  if (pathStr.empty()) {
    return makeFuture<InodePtr>(inodePtrFromThis());
  }

  auto processor = std::make_unique<LookupProcessor>(path);
  auto future = processor->next(inodePtrFromThis());
  // This ensure() callback serves to hold onto the unique_ptr,
  // and makes sure it only gets destroyed when the future is finally resolved.
  return std::move(future).ensure(
      [p = std::move(processor)]() mutable { p.reset(); });
}

InodeNumber TreeInode::getChildInodeNumber(PathComponentPiece name) {
  auto contents = contents_.wlock();
  auto iter = contents->entries.find(name);
  if (iter == contents->entries.end()) {
    throw InodeError(ENOENT, inodePtrFromThis(), name);
  }

  auto& ent = iter->second;
  DCHECK(!ent.getInode() || ent.getInode()->getNodeId() == ent.getInodeNumber())
      << "inode number mismatch: " << ent.getInode()->getNodeId()
      << " != " << ent.getInodeNumber();
  return ent.getInodeNumber();
}

void TreeInode::loadUnlinkedChildInode(
    PathComponentPiece name,
    InodeNumber number,
    std::optional<Hash> hash,
    mode_t mode) {
  try {
    InodeMap::PromiseVector promises;
    InodePtr inodePtr;

    if (!S_ISDIR(mode)) {
      auto file = std::make_unique<FileInode>(
          number, inodePtrFromThis(), name, mode, std::nullopt, hash);
      promises = getInodeMap()->inodeLoadComplete(file.get());
      inodePtr = InodePtr::takeOwnership(std::move(file));
    } else {
      auto overlayContents = getOverlay()->loadOverlayDir(number);
      if (!hash) {
        // If the inode is materialized, the overlay must have an entry
        // for the directory.
        // Note that the .value() call will throw if we couldn't
        // load the dir data; we'll catch and propagate that in
        // the containing try/catch block.
        if (!overlayContents.value().empty()) {
          // Should be impossible, but worth checking for
          // defensive purposes!
          throw new std::runtime_error(
              "unlinked dir inode should have no children");
        }
      }

      auto tree = std::make_unique<TreeInode>(
          number,
          inodePtrFromThis(),
          name,
          mode,
          std::nullopt,
          std::move(overlayContents).value_or(DirContents{}),
          hash);
      promises = getInodeMap()->inodeLoadComplete(tree.get());
      inodePtr = InodePtr::takeOwnership(std::move(tree));
    }

    inodePtr->markUnlinkedAfterLoad();

    // Alert any waiters that the load is complete
    for (auto& promise : promises) {
      promise.setValue(inodePtr);
    }

  } catch (const std::exception& exc) {
    auto bug = EDEN_BUG_EXCEPTION()
        << "InodeMap requested to load inode " << number << "(" << name
        << " in " << getLogPath()
        << "), which has been unlinked, and we hit this "
        << "error while trying to load it from the overlay: " << exc.what();
    getInodeMap()->inodeLoadFailed(number, bug);
  }
}

void TreeInode::loadChildInode(PathComponentPiece name, InodeNumber number) {
  auto future = Future<unique_ptr<InodeBase>>::makeEmpty();
  {
    auto contents = contents_.rlock();
    auto iter = contents->entries.find(name);
    if (iter == contents->entries.end()) {
      auto bug = EDEN_BUG_EXCEPTION()
          << "InodeMap requested to load inode " << number
          << ", but there is no entry named \"" << name << "\" in "
          << getNodeId();
      getInodeMap()->inodeLoadFailed(number, bug);
      return;
    }

    auto& entry = iter->second;
    // InodeMap makes sure to only try loading each inode once, so this entry
    // should not already be loaded.
    if (entry.getInode() != nullptr) {
      auto bug = EDEN_BUG_EXCEPTION()
          << "InodeMap requested to load inode " << number << "(" << name
          << " in " << getNodeId() << "), which is already loaded";
      // Call inodeLoadFailed().  (Arguably we could call inodeLoadComplete()
      // if the existing inode has the same number as the one we were requested
      // to load.  However, it seems more conservative to just treat this as
      // failed and fail pending promises waiting on this inode.  This may
      // cause problems for anyone trying to access this child inode in the
      // future, but at least it shouldn't damage the InodeMap data structures
      // any further.)
      getInodeMap()->inodeLoadFailed(number, bug);
      return;
    }

    future = startLoadingInodeNoThrow(entry, name);
  }
  registerInodeLoadComplete(future, name, number);
}

void TreeInode::registerInodeLoadComplete(
    folly::Future<unique_ptr<InodeBase>>& future,
    PathComponentPiece name,
    InodeNumber number) {
  // This method should never be called with the contents_ lock held.  If the
  // future is already ready we will try to acquire the contents_ lock now.
  std::move(future)
      .thenValue([self = inodePtrFromThis(), childName = PathComponent{name}](
                     unique_ptr<InodeBase>&& childInode) {
        self->inodeLoadComplete(childName, std::move(childInode));
      })
      .thenError([self = inodePtrFromThis(),
                  number](const folly::exception_wrapper& ew) {
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
      XLOG(ERR) << "child " << childName << " in " << getLogPath()
                << " removed before it finished loading";
      throw InodeError(
          ENOENT,
          inodePtrFromThis(),
          childName,
          "inode removed before loading finished");
    }
    iter->second.setInode(childInode.get());
    // Make sure that we are still holding the contents_ lock when
    // calling inodeLoadComplete().  This ensures that no-one can look up
    // the inode by name before it is also available in the InodeMap.
    // However, we must wait to fulfill pending promises until after
    // releasing our lock.
    promises = getInodeMap()->inodeLoadComplete(childInode.get());
  }

  // Fulfill all of the pending promises after releasing our lock
  auto inodePtr = InodePtr::takeOwnership(std::move(childInode));
  for (auto& promise : promises) {
    promise.setValue(inodePtr);
  }
}

Future<unique_ptr<InodeBase>> TreeInode::startLoadingInodeNoThrow(
    const DirEntry& entry,
    PathComponentPiece name) noexcept {
  // The callers of startLoadingInodeNoThrow() need to make sure that they
  // always call InodeMap::inodeLoadComplete() or InodeMap::inodeLoadFailed()
  // afterwards.
  //
  // It simplifies their logic to guarantee that we never throw an exception,
  // and always return a Future object.  Therefore we simply wrap
  // startLoadingInode() and convert any thrown exceptions into Future.
  try {
    return startLoadingInode(entry, name);
  } catch (const std::exception& ex) {
    // It's possible that makeFuture() itself could throw, but this only
    // happens on out of memory, in which case the whole process is pretty much
    // hosed anyway.
    return makeFuture<unique_ptr<InodeBase>>(
        folly::exception_wrapper{std::current_exception(), ex});
  }
}

template <typename T>
inline std::ostream& operator<<(
    std::ostream& os,
    const std::optional<T>& value) {
  if (value) {
    return os << "some(" << *value << ")";
  } else {
    return os << "none";
  }
}

static std::vector<std::string> computeEntryDifferences(
    const DirContents& dir,
    const Tree& tree) {
  std::set<std::string> differences;
  for (const auto& entry : dir) {
    if (!tree.getEntryPtr(entry.first)) {
      differences.insert("- " + entry.first.stringPiece().str());
    }
  }
  for (const auto& entry : tree.getTreeEntries()) {
    if (!dir.count(entry.getName())) {
      differences.insert("+ " + entry.getName().stringPiece().str());
    }
  }
  return std::vector<std::string>{differences.begin(), differences.end()};
}

std::optional<std::vector<std::string>> findEntryDifferences(
    const DirContents& dir,
    const Tree& tree) {
  // Avoid allocations in the case where the tree and dir agree.
  if (dir.size() != tree.getTreeEntries().size()) {
    return computeEntryDifferences(dir, tree);
  }
  for (const auto& entry : dir) {
    if (!tree.getEntryPtr(entry.first)) {
      return computeEntryDifferences(dir, tree);
    }
  }
  return std::nullopt;
}

Future<unique_ptr<InodeBase>> TreeInode::startLoadingInode(
    const DirEntry& entry,
    PathComponentPiece name) {
  XLOG(DBG5) << "starting to load inode " << entry.getInodeNumber() << ": "
             << getLogPath() << " / \"" << name << "\"";
  DCHECK(entry.getInode() == nullptr);
  if (!entry.isDirectory()) {
    // If this is a file we can just go ahead and create it now;
    // we don't need to load anything else.
    //
    // Eventually we may want to go ahead start loading some of the blob data
    // now, but we don't have to wait for it to be ready before marking the
    // inode loaded.
    return make_unique<FileInode>(
        entry.getInodeNumber(),
        inodePtrFromThis(),
        name,
        entry.getInitialMode(),
        std::nullopt,
        entry.getOptionalHash());
  }

  if (!entry.isMaterialized()) {
    return getStore()
        ->getTree(entry.getHash())
        .thenValue(
            [self = inodePtrFromThis(),
             childName = PathComponent{name},
             treeHash = entry.getHash(),
             entryMode = entry.getInitialMode(),
             number = entry.getInodeNumber()](
                std::shared_ptr<const Tree> tree) mutable
            -> unique_ptr<InodeBase> {
              // Even if the inode is not materialized, it may have inode
              // numbers stored in the overlay.
              auto overlayDir = self->loadOverlayDir(number);
              if (overlayDir) {
                // Compare the Tree and the Dir from the overlay.  If they
                // differ, something is wrong, so log the difference.
                if (auto differences =
                        findEntryDifferences(*overlayDir, *tree)) {
                  std::string diffString;
                  for (const auto& diff : *differences) {
                    diffString += diff;
                    diffString += '\n';
                  }
                  XLOG(ERR)
                      << "loaded entry " << self->getLogPath() << " / "
                      << childName << " (inode number " << number
                      << ") from overlay but the entries don't correspond with "
                         "the tree.  Something is wrong!\n"
                      << diffString;
                }

                XLOG(DBG6) << "found entry " << childName
                           << " with inode number " << number << " in overlay";
                return make_unique<TreeInode>(
                    number,
                    std::move(self),
                    childName,
                    entryMode,
                    std::nullopt,
                    std::move(*overlayDir),
                    treeHash);
              }

              return make_unique<TreeInode>(
                  number, self, childName, entryMode, std::move(tree));
            });
  }

  // The entry is materialized, so data must exist in the overlay.
  auto overlayDir = loadOverlayDir(entry.getInodeNumber());
  if (!overlayDir) {
    return EDEN_BUG_FUTURE(unique_ptr<InodeBase>)
        << "missing overlay for " << getLogPath() << " / " << name;
  }
  return make_unique<TreeInode>(
      entry.getInodeNumber(),
      inodePtrFromThis(),
      name,
      entry.getInitialMode(),
      std::nullopt,
      std::move(*overlayDir),
      std::nullopt);
}

void TreeInode::materialize(const RenameLock* renameLock) {
  // If we don't have the rename lock yet, do a quick check first
  // to avoid acquiring it if we don't actually need to change anything.
  if (!renameLock) {
    auto contents = contents_.rlock();
    if (contents->isMaterialized()) {
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
      if (contents->isMaterialized()) {
        return;
      }
      contents->setMaterialized();
      saveOverlayDir(contents->entries);
    }

    // Mark ourself materialized in our parent directory (if we have one)
    auto loc = getLocationInfo(*renameLock);
    if (loc.parent && !loc.unlinked) {
      loc.parent->childMaterialized(*renameLock, loc.name);
    }
  }
}

/* If we don't yet have an overlay entry for this portion of the tree,
 * populate it from the Tree.  In order to materialize a dir we have
 * to also materialize its parents. */
void TreeInode::childMaterialized(
    const RenameLock& renameLock,
    PathComponentPiece childName) {
  {
    auto contents = contents_.wlock();
    auto iter = contents->entries.find(childName);
    if (iter == contents->entries.end()) {
      // This should never happen.
      // We should only get called with legitimate children names.
      EDEN_BUG() << "error attempting to materialize " << childName << " in "
                 << getLogPath() << ": entry not present";
    }

    auto& childEntry = iter->second;
    if (contents->isMaterialized() && childEntry.isMaterialized()) {
      // Nothing to do
      return;
    }

    childEntry.setMaterialized();
    contents->setMaterialized();
    saveOverlayDir(contents->entries);
  }

  // If we have a parent directory, ask our parent to materialize itself
  // and mark us materialized when it does so.
  auto location = getLocationInfo(renameLock);
  if (location.parent && !location.unlinked) {
    location.parent->childMaterialized(renameLock, location.name);
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

    auto& childEntry = iter->second;
    if (!childEntry.isMaterialized() && childEntry.getHash() == childScmHash) {
      // Nothing to do.  Our child's state and our own are both unchanged.
      return;
    }

    // Mark the child dematerialized.
    childEntry.setDematerialized(childScmHash);

    // Mark us materialized!
    //
    // Even though our child is dematerialized, we always materialize ourself
    // so we make sure we record the correct source control hash for our child.
    // Currently dematerialization only happens on the checkout() flow.  Once
    // checkout finishes processing all of the children it will call
    // saveOverlayPostCheckout() on this directory, and here we will check to
    // see if we can dematerialize ourself.
    contents->setMaterialized();
    saveOverlayDir(contents->entries);
  }

  // We are materialized now.
  // If we have a parent directory, ask our parent to materialize itself
  // and mark us materialized when it does so.
  auto location = getLocationInfo(renameLock);
  if (location.parent && !location.unlinked) {
    location.parent->childMaterialized(renameLock, location.name);
  }
}

Overlay* TreeInode::getOverlay() const {
  return getMount()->getOverlay();
}

std::optional<DirContents> TreeInode::loadOverlayDir(
    InodeNumber inodeNumber) const {
  return getOverlay()->loadOverlayDir(inodeNumber);
}

void TreeInode::saveOverlayDir(const DirContents& contents) const {
  return saveOverlayDir(getNodeId(), contents);
}

void TreeInode::saveOverlayDir(
    InodeNumber inodeNumber,
    const DirContents& contents) const {
  return getOverlay()->saveOverlayDir(inodeNumber, contents);
}

DirContents TreeInode::saveDirFromTree(
    InodeNumber inodeNumber,
    const Tree* tree,
    EdenMount* mount) {
  auto overlay = mount->getOverlay();
  auto dir = buildDirFromTree(tree, overlay);
  // buildDirFromTree just allocated inode numbers; they should be saved.
  overlay->saveOverlayDir(inodeNumber, dir);
  return dir;
}

DirContents TreeInode::buildDirFromTree(const Tree* tree, Overlay* overlay) {
  CHECK(tree);

  // A future optimization is for this code to allocate all of the inode numbers
  // at once and then dole them out, one per entry. It would reduce the number
  // of atomic operations from N to 1, though if the atomic is issued with the
  // other work this loop is doing it may not matter much.

  DirContents dir;
  // TODO: O(N^2)
  for (const auto& treeEntry : tree->getTreeEntries()) {
    dir.emplace(
        treeEntry.getName(),
        modeFromTreeEntryType(treeEntry.getType()),
        overlay->allocateInodeNumber(),
        treeEntry.getHash());
  }
  return dir;
}

FileInodePtr TreeInode::createImpl(
    folly::Synchronized<TreeInodeState>::LockedPtr contents,
    PathComponentPiece name,
    mode_t mode,
    ByteRange fileContents) {
  // This relies on the fact that the dotEdenInodeNumber field of EdenMount is
  // not defined until after EdenMount finishes configuring the .eden directory.
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis(), name);
  }

  FileInodePtr inode;
  RelativePath targetName;

  // New scope to distinguish work done with the contents lock and to help
  // manage releasing it.
  {
    // Ensure that we always unlock contents at the end of this scope.
    // Even if an exception is thrown we need to make sure we release the
    // contents lock before the local inode variable gets destroyed.
    // If an error is thrown, destroying the inode may attempt to acquire the
    // parents contents lock, which will block if we are still holding it.
    // (T42835005).
    SCOPE_EXIT {
      contents.unlock();
    };

    // Make sure that an entry with this name does not already exist.
    //
    // In general FUSE should avoid calling create(), symlink(), or mknod() on
    // entries that already exist.  It performs its own check in the kernel
    // first to see if this entry exists.  However, this may race with a
    // checkout operation, so it is still possible that it calls us with an
    // entry that was in fact just created by a checkout operation.
    auto entIter = contents->entries.find(name);
    if (entIter != contents->entries.end()) {
      throw InodeError(EEXIST, this->inodePtrFromThis(), name);
    }

    auto myPath = getPath();
    // Make sure this directory has not been unlinked.
    // We have to check this after acquiring the contents_ lock; otherwise
    // we could race with rmdir() or rename() calls affecting us.
    if (!myPath.has_value()) {
      throw InodeError(ENOENT, inodePtrFromThis());
    }

    // Compute the target path, so we can record it in the journal below
    // after releasing the contents lock.
    targetName = myPath.value() + name;

    // Generate an inode number for this new entry.
    auto childNumber = getOverlay()->allocateInodeNumber();

    // Create the overlay file before we insert the file into our entries map.
    auto file = getOverlay()->createOverlayFile(childNumber, fileContents);

    auto now = getNow();
    auto inodeTimestamps = InodeTimestamps{now};

    // Record the new entry
    auto insertion = contents->entries.emplace(name, mode, childNumber);
    CHECK(insertion.second)
        << "we already confirmed that this entry did not exist above";
    auto& entry = insertion.first->second;

    inode = FileInodePtr::makeNew(
        childNumber, this->inodePtrFromThis(), name, mode, inodeTimestamps);

    entry.setInode(inode.get());
    getInodeMap()->inodeCreated(inode);

    updateMtimeAndCtimeLocked(contents->entries, now);
    getMount()->getServerState()->getFaultInjector().check(
        "createInodeSaveOverlay", name.stringPiece());
    saveOverlayDir(contents->entries);
  }

  invalidateFuseEntryCacheIfRequired(name);
  invalidateFuseInodeCacheIfRequired();

  getMount()->getJournal().recordCreated(targetName);

  return inode;
}

FileInodePtr TreeInode::symlink(
    PathComponentPiece name,
    folly::StringPiece symlinkTarget) {
  validatePathComponentLength(name);
  materialize();

  {
    // Acquire our contents lock
    auto contents = contents_.wlock();
    const mode_t mode = S_IFLNK | 0770;
    return createImpl(
        std::move(contents), name, mode, ByteRange{symlinkTarget});
  }
}

FileInodePtr TreeInode::mknod(PathComponentPiece name, mode_t mode, dev_t dev) {
  validatePathComponentLength(name);

  // Compute the effective name of the node they want to create.
  RelativePath targetName;
  FileInodePtr inode;

  if (!S_ISSOCK(mode) && !S_ISREG(mode)) {
    throw InodeError(
        EPERM,
        inodePtrFromThis(),
        name,
        "only unix domain sockets and regular files are supported by mknod");
  }

  // The dev parameter to mknod only applies to block and character devices,
  // which edenfs does not support today.  Therefore, we do not need to store
  // it.  If we add block device support in the future, makes sure dev makes it
  // into the FileInode and directory entry.
  (void)dev;

  materialize();

  {
    // Acquire our contents lock
    auto contents = contents_.wlock();
    return createImpl(std::move(contents), name, mode, ByteRange{});
  }
}

TreeInodePtr TreeInode::mkdir(PathComponentPiece name, mode_t mode) {
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis(), name);
  }
  validatePathComponentLength(name);

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
    if (!myPath.has_value()) {
      throw InodeError(ENOENT, inodePtrFromThis());
    }
    // Compute the target path, so we can record it in the journal below.
    targetName = myPath.value() + name;

    auto entIter = contents->entries.find(name);
    if (entIter != contents->entries.end()) {
      throw InodeError(EEXIST, this->inodePtrFromThis(), name);
    }

    // Allocate an inode number
    auto childNumber = getOverlay()->allocateInodeNumber();

    // The mode passed in by the caller may not have the file type bits set.
    // Ensure that we mark this as a directory.
    mode = S_IFDIR | (07777 & mode);

    // Store the overlay entry for this dir
    DirContents emptyDir;
    saveOverlayDir(childNumber, emptyDir);

    // Add a new entry to contents_.entries
    auto emplaceResult = contents->entries.emplace(name, mode, childNumber);
    CHECK(emplaceResult.second)
        << "directory contents should not have changed since the check above";
    auto& entry = emplaceResult.first->second;

    // Update timeStamps of newly created directory and current directory.
    auto now = getNow();
    newChild = TreeInodePtr::makeNew(
        childNumber,
        this->inodePtrFromThis(),
        name,
        mode,
        InodeTimestamps{now},
        std::move(emptyDir),
        std::nullopt);
    entry.setInode(newChild.get());
    getInodeMap()->inodeCreated(newChild);

    // Save our updated overlay data
    updateMtimeAndCtimeLocked(contents->entries, now);
    saveOverlayDir(contents->entries);
  }

  invalidateFuseEntryCacheIfRequired(name);
  invalidateFuseInodeCacheIfRequired();
  getMount()->getJournal().recordCreated(targetName);

  return newChild;
}

folly::Future<folly::Unit> TreeInode::unlink(PathComponentPiece name) {
  return getOrLoadChild(name).thenValue(
      [self = inodePtrFromThis(),
       childName = PathComponent{name}](const InodePtr& child) {
        return self->removeImpl<FileInodePtr>(std::move(childName), child, 1);
      });
}

folly::Future<folly::Unit> TreeInode::rmdir(PathComponentPiece name) {
  return getOrLoadChild(name).thenValue(
      [self = inodePtrFromThis(),
       childName = PathComponent{name}](const InodePtr& child) {
        return self->removeImpl<TreeInodePtr>(std::move(childName), child, 1);
      });
}

template <typename InodePtrType>
folly::Future<folly::Unit> TreeInode::removeImpl(
    PathComponent name,
    InodePtr childBasePtr,
    unsigned int attemptNum) {
  // Make sure the child is of the desired type
  auto child = childBasePtr.asSubclassPtrOrNull<InodePtrType>();
  if (!child) {
    return makeFuture<Unit>(
        InodeError(InodePtrType::InodeType::WRONG_TYPE_ERRNO, childBasePtr));
  }

  // Verify that we can remove the child before we materialize ourself
  int checkResult = checkPreRemove(child);
  if (checkResult != 0) {
    return makeFuture<Unit>(InodeError(checkResult, child));
  }

  // Acquire the rename lock since we need to update our child's location
  auto renameLock = getMount()->acquireRenameLock();

  // Get the path to the child, so we can update the journal later.
  // Make sure we only do this after we acquire the rename lock, so that the
  // path reported in the journal will be accurate.
  auto myPath = getPath();
  if (!myPath.has_value()) {
    // It appears we have already been unlinked.  It's possible someone other
    // thread has already renamed child to another location and unlinked us.
    // Just fail with ENOENT in this case.
    return makeFuture<Unit>(InodeError(ENOENT, inodePtrFromThis()));
  }
  auto targetName = myPath.value() + name;

  // The entry in question may have been renamed since we loaded the child
  // Inode pointer.  If this happens, that's fine, and we just want to go ahead
  // and try removing whatever is present with this name anyway.
  //
  // Therefore leave the child parameter for tryRemoveChild() as null, and let
  // it remove whatever it happens to find with this name.
  const InodePtrType nullChildPtr;
  // Set the flushKernelCache parameter to true unless this was triggered by a
  // FUSE request, in which case the kernel will automatically update its
  // cache correctly.
  bool flushKernelCache = !RequestData::isFuseRequest();
  int errnoValue =
      tryRemoveChild(renameLock, name, nullChildPtr, flushKernelCache);
  if (errnoValue == 0) {
    // We successfully removed the child.
    // Record the change in the journal.
    getMount()->getJournal().recordRemoved(targetName);

    return folly::unit;
  }

  // EBADF means that the child in question has been replaced since we looked
  // it up earlier, and the child inode now at this location is not loaded.
  if (errnoValue != EBADF) {
    return makeFuture<Unit>(InodeError(errnoValue, inodePtrFromThis(), name));
  }

  // Give up after 3 retries
  constexpr unsigned int kMaxRemoveRetries = 3;
  if (attemptNum > kMaxRemoveRetries) {
    throw InodeError(
        EIO,
        inodePtrFromThis(),
        name,
        "inode was removed/renamed after remove started");
  }

  // Note that we intentially create childFuture() in a separate
  // statement before calling thenValue() on it, since we std::move()
  // the name into the lambda capture for thenValue().
  //
  // Pre-C++17 this has undefined behavior if they are both in the same
  // statement: argument evaluation order is undefined, so we could
  // create the lambda (and invalidate name) before calling
  // getOrLoadChildTree(name).  C++17 fixes this order to guarantee that
  // the left side of "." will always get evaluated before the right
  // side.
  auto childFuture = getOrLoadChild(name);
  return std::move(childFuture)
      .thenValue([self = inodePtrFromThis(),
                  childName = PathComponent{std::move(name)},
                  attemptNum](const InodePtr& loadedChild) {
        return self->removeImpl<InodePtrType>(
            childName, loadedChild, attemptNum + 1);
      });
}

template <typename InodePtrType>
int TreeInode::tryRemoveChild(
    const RenameLock& renameLock,
    PathComponentPiece name,
    InodePtrType child,
    bool flushKernelCache) {
  materialize(&renameLock);

  // prevent unlinking files in the .eden directory
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return EPERM;
  }

  // Lock our contents in write mode.
  // We will hold it for the duration of the unlink.
  std::unique_ptr<InodeBase> deletedInode;
  {
    auto contents = contents_.wlock();

    // Make sure that this name still corresponds to the child inode we just
    // looked up.
    auto entIter = contents->entries.find(name);
    if (entIter == contents->entries.end()) {
      return ENOENT;
    }
    auto& ent = entIter->second;
    if (!ent.getInode()) {
      // The inode in question is not loaded.  The caller will need to load it
      // and retry (if they want to retry).
      return EBADF;
    }
    if (child) {
      if (ent.getInode() != child.get()) {
        // This entry no longer refers to what the caller expected.
        return EBADF;
      }
    } else {
      // Make sure the entry being removed is the expected file/directory type.
      child = ent.getInodePtr().asSubclassPtrOrNull<InodePtrType>();
      if (!child) {
        return InodePtrType::InodeType::WRONG_TYPE_ERRNO;
      }
    }

    // Verify that the child is still in a good state to remove
    auto checkError = checkPreRemove(child);
    if (checkError != 0) {
      return checkError;
    }

    // Inform the child it is now unlinked
    deletedInode = child->markUnlinked(this, name, renameLock);

    // Remove it from our entries list
    contents->entries.erase(entIter);

    // We want to update mtime and ctime of parent directory after removing the
    // child.
    updateMtimeAndCtimeLocked(contents->entries, getNow());
    saveOverlayDir(contents->entries);
  }
  deletedInode.reset();

  // We have successfully removed the entry.
  // Flush the kernel cache for this entry if requested.
  if (flushKernelCache) {
    invalidateFuseInodeCache();
    invalidateFuseEntryCache(name);
  }

  return 0;
}

int TreeInode::checkPreRemove(const TreeInodePtr& child) {
  // Lock the child contents, and make sure they are empty
  auto childContents = child->contents_.rlock();
  if (!childContents->entries.empty()) {
    return ENOTEMPTY;
  }
  return 0;
}

int TreeInode::checkPreRemove(const FileInodePtr& /* child */) {
  // Nothing to do
  return 0;
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

  /**
   * Reset the TreeRenameLocks to the empty state, releasing all locks that it
   * holds.
   */
  void reset() {
    *this = TreeRenameLocks();
  }

  /**
   * Release all locks held by this TreeRenameLocks object except for the
   * mount point RenameLock.
   */
  void releaseAllButRename() {
    *this = TreeRenameLocks(std::move(renameLock_));
  }

  const RenameLock& renameLock() const {
    return renameLock_;
  }

  DirContents* srcContents() {
    return srcContents_;
  }

  DirContents* destContents() {
    return destContents_;
  }

  const PathMap<DirEntry>::iterator& destChildIter() const {
    return destChildIter_;
  }
  InodeBase* destChild() const {
    DCHECK(destChildExists());
    return destChildIter_->second.getInode();
  }

  bool destChildExists() const {
    return destChildIter_ != destContents_->end();
  }
  bool destChildIsDirectory() const {
    DCHECK(destChildExists());
    return destChildIter_->second.isDirectory();
  }
  bool destChildIsEmpty() const {
    DCHECK(destChildContents_);
    return destChildContents_->empty();
  }

 private:
  explicit TreeRenameLocks(RenameLock&& renameLock)
      : renameLock_{std::move(renameLock)} {}

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
  folly::Synchronized<TreeInodeState>::LockedPtr srcContentsLock_;
  folly::Synchronized<TreeInodeState>::LockedPtr destContentsLock_;
  folly::Synchronized<TreeInodeState>::LockedPtr destChildContentsLock_;

  /**
   * Pointers to the source and destination directory contents.
   *
   * These may both point to the same contents when the source and destination
   * directory are the same.
   */
  DirContents* srcContents_{nullptr};
  DirContents* destContents_{nullptr};
  DirContents* destChildContents_{nullptr};

  /**
   * An iterator pointing to the destination child entry in
   * destContents_->entries.
   * This may point to destContents_->entries.end() if the destination child
   * does not exist.
   */
  PathMap<DirEntry>::iterator destChildIter_;
};

Future<Unit> TreeInode::rename(
    PathComponentPiece name,
    TreeInodePtr destParent,
    PathComponentPiece destName) {
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return makeFuture<Unit>(InodeError(EPERM, inodePtrFromThis(), name));
  }
  if (destParent->getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return makeFuture<Unit>(InodeError(EPERM, destParent, destName));
  }
  validatePathComponentLength(destName);

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
    auto srcIter = locks.srcContents()->find(name);
    if (srcIter == locks.srcContents()->end()) {
      // The source path does not exist.  Fail the rename.
      return makeFuture<Unit>(InodeError(ENOENT, inodePtrFromThis(), name));
    }
    DirEntry& srcEntry = srcIter->second;

    // Perform as much input validation as possible now, before starting inode
    // loads that might be necessary.

    // Validate invalid file/directory replacement
    if (srcEntry.isDirectory()) {
      // The source is a directory.
      // The destination must not exist, or must be an empty directory,
      // or the exact same directory.
      if (locks.destChildExists()) {
        if (!locks.destChildIsDirectory()) {
          XLOG(DBG4) << "attempted to rename directory " << getLogPath() << "/"
                     << name << " over file " << destParent->getLogPath() << "/"
                     << destName;
          return makeFuture<Unit>(InodeError(ENOTDIR, destParent, destName));
        } else if (
            locks.destChild() != srcEntry.getInode() &&
            !locks.destChildIsEmpty()) {
          XLOG(DBG4) << "attempted to rename directory " << getLogPath() << "/"
                     << name << " over non-empty directory "
                     << destParent->getLogPath() << "/" << destName;
          return makeFuture<Unit>(InodeError(ENOTEMPTY, destParent, destName));
        }
      }
    } else {
      // The source is not a directory.
      // The destination must not exist, or must not be a directory.
      if (locks.destChildExists() && locks.destChildIsDirectory()) {
        XLOG(DBG4) << "attempted to rename file " << getLogPath() << "/" << name
                   << " over directory " << destParent->getLogPath() << "/"
                   << destName;
        return makeFuture<Unit>(InodeError(EISDIR, destParent, destName));
      }
    }

    // Make sure the destination directory is not unlinked.
    if (destParent->isUnlinked()) {
      XLOG(DBG4) << "attempted to rename file " << getLogPath() << "/" << name
                 << " into deleted directory " << destParent->getLogPath()
                 << " ( as " << destName << ")";
      return makeFuture<Unit>(InodeError(ENOENT, destParent));
    }

    // Check to see if we need to load the source or destination inodes
    needSrc = !srcEntry.getInode();
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
  auto onLoadFinished = [self = inodePtrFromThis(),
                         nameCopy = name.copy(),
                         destParent,
                         destNameCopy = destName.copy()](auto&&) {
    return self->rename(nameCopy, destParent, destNameCopy);
  };

  if (needSrc && needDest) {
    auto srcFuture = getOrLoadChild(name);
    auto destFuture = destParent->getOrLoadChild(destName);
    return folly::collect(srcFuture, destFuture).thenValue(onLoadFinished);
  } else if (needSrc) {
    return getOrLoadChild(name).thenValue(onLoadFinished);
  } else {
    CHECK(needDest);
    return destParent->getOrLoadChild(destName).thenValue(onLoadFinished);
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
} // namespace

Future<Unit> TreeInode::doRename(
    TreeRenameLocks&& locks,
    PathComponentPiece srcName,
    PathMap<DirEntry>::iterator srcIter,
    TreeInodePtr destParent,
    PathComponentPiece destName) {
  DirEntry& srcEntry = srcIter->second;

  // If the source and destination refer to exactly the same file,
  // then just succeed immediately.  Nothing needs to be done in this case.
  if (locks.destChildExists() && srcEntry.getInode() == locks.destChild()) {
    return folly::unit;
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
  if (srcEntry.isDirectory()) {
    // Our caller has already verified that the source is also a
    // directory here.
    auto* srcTreeInode =
        boost::polymorphic_downcast<TreeInode*>(srcEntry.getInode());
    if (srcTreeInode == destParent.get() ||
        isAncestor(locks.renameLock(), srcTreeInode, destParent.get())) {
      return makeFuture<Unit>(InodeError(EINVAL, destParent, destName));
    }
  }

  // Success.
  // Update the destination with the source data (this copies in the hash if
  // it happens to be set).
  std::unique_ptr<InodeBase> deletedInode;
  auto* childInode = srcEntry.getInode();
  bool destChildExists = locks.destChildExists();
  if (destChildExists) {
    deletedInode = locks.destChild()->markUnlinked(
        destParent.get(), destName, locks.renameLock());

    // Replace the destination contents entry with the source data
    locks.destChildIter()->second = std::move(srcIter->second);
  } else {
    auto ret =
        locks.destContents()->emplace(destName, std::move(srcIter->second));
    CHECK(ret.second);

    // If the source and destination directory are the same, then inserting the
    // destination entry may have invalidated our source entry iterator, so we
    // have to look it up again.
    if (destParent.get() == this) {
      srcIter = locks.srcContents()->find(srcName);
    }
  }

  // Inform the child inode that it has been moved
  childInode->updateLocation(destParent, destName, locks.renameLock());

  // Now remove the source information
  locks.srcContents()->erase(srcIter);

  auto now = getNow();
  updateMtimeAndCtimeLocked(*locks.srcContents(), now);
  if (destParent.get() != this) {
    destParent->updateMtimeAndCtimeLocked(*locks.destContents(), now);
  }

  // Save the overlay data
  saveOverlayDir(*locks.srcContents());
  if (destParent.get() != this) {
    saveOverlayDir(destParent->getNodeId(), *locks.destContents());
  }

  // Release the TreeInode locks before we write a journal entry.
  // We keep holding the mount point rename lock for now though.  This ensures
  // that rename and deletion events do show up in the journal in the correct
  // order.
  locks.releaseAllButRename();

  // Add a journal entry
  auto srcPath = getPath();
  auto destPath = destParent->getPath();
  if (srcPath.has_value() && destPath.has_value()) {
    if (destChildExists) {
      getMount()->getJournal().recordReplaced(
          srcPath.value() + srcName, destPath.value() + destName);
    } else {
      getMount()->getJournal().recordRenamed(
          srcPath.value() + srcName, destPath.value() + destName);
    }
  }

  // Release the rename lock before we destroy the deleted destination child
  // inode (if it exists).
  locks.reset();
  deletedInode.reset();

  // If the rename occurred outside of a FUSE request (unlikely), make sure to
  // invalidate the kernel caches.
  invalidateFuseInodeCacheIfRequired();
  if (destParent.get() != this) {
    destParent->invalidateFuseInodeCacheIfRequired();
  }
  invalidateFuseEntryCacheIfRequired(srcName);
  destParent->invalidateFuseEntryCacheIfRequired(destName);

  return folly::unit;
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
    srcContents_ = &srcContentsLock_->entries;
    destContents_ = &srcContentsLock_->entries;
    // Look up the destination child entry, and lock it if is is a directory
    lockDestChild(destName);
  } else if (isAncestor(renameLock_, srcTree, destTree)) {
    // If srcTree is an ancestor of destTree, we must acquire the lock on
    // srcTree first.
    srcContentsLock_ = srcTree->contents_.wlock();
    srcContents_ = &srcContentsLock_->entries;
    destContentsLock_ = destTree->contents_.wlock();
    destContents_ = &destContentsLock_->entries;
    lockDestChild(destName);
  } else {
    // In all other cases, lock destTree and destChild before srcTree,
    // as long as we verify that destChild and srcTree are not the same.
    //
    // It is not possible for srcTree to be an ancestor of destChild,
    // since we have confirmed that srcTree is not destTree nor an ancestor of
    // destTree.
    destContentsLock_ = destTree->contents_.wlock();
    destContents_ = &destContentsLock_->entries;
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
      srcContents_ = &srcContentsLock_->entries;
    }
  }
}

void TreeInode::TreeRenameLocks::lockDestChild(PathComponentPiece destName) {
  // Look up the destination child entry
  destChildIter_ = destContents_->find(destName);
  if (destChildExists() && destChildIsDirectory() && destChild() != nullptr) {
    auto* childTree = boost::polymorphic_downcast<TreeInode*>(destChild());
    destChildContentsLock_ = childTree->contents_.wlock();
    destChildContents_ = &destChildContentsLock_->entries;
  }
}

DirList TreeInode::readdir(DirList&& list, off_t off) {
  /*
   * Implementing readdir correctly in the presence of concurrent modifications
   * to the directory is nontrivial. This function will be called multiple
   * times. The off_t value given is either 0, on the first read, or the value
   * corresponding to the last entry's offset. (Or an arbitrary entry's offset
   * value, given seekdir and telldir).
   *
   * POSIX compliance requires that, given a sequence of readdir calls across
   * the an entire directory stream, all entries that are not modified are
   * returned exactly once. Entries that are added or removed between readdir
   * calls may be returned, but don't have to be.
   *
   * Thus, off_t as an index into an ordered list of entries is not sufficient.
   * If an entry is unlinked, the next readdir will skip entries.
   *
   * One option might be to populate off_t with a hash of the entry name. off_t
   * has 63 usable bits (minus the 0 value which is reserved for the initial
   * request). 63 bits of SpookyHashV2 is probably sufficient in practice, but
   * it would be possible to create a directory containing collisions, causing
   * duplicate entries or an infinite loop. Also it's unclear how to handle
   * the entry at `off` being removed before the next readdir. (How do you find
   * where to restart in the stream?).
   *
   * Today, Eden does not support hard links. Therefore, in the short term, we
   * can store inode numbers in off_t and treat them as an index into an
   * inode-sorted list of entries. This has quadratic time complexity without an
   * additional index but is correct.
   *
   * In the long term, especially when Eden's tree directory structure is stored
   * in SQLite or something similar, we should maintain a seekdir/readdir cookie
   * index and use said cookies to enumerate entries.
   *
   * - https://oss.oracle.com/pipermail/btrfs-devel/2008-January/000463.html
   * - https://yarchive.net/comp/linux/readdir_nonatomicity.html
   * - https://lwn.net/Articles/544520/
   */
  if (off < 0) {
    XLOG(ERR) << "Negative readdir offsets are illegal, off = " << off;
    folly::throwSystemErrorExplicit(EINVAL);
  }
  updateAtime();

  // It's very common for userspace to readdir() a directory to completion and
  // serially stat() every entry. Since stat() returns a file's size and a
  // directory's entry count in the st_nlink field, upon the first readdir for a
  // given inode, fetch metadata for each entry in parallel. prefetch() may
  // return early if the metadata for this inode's children has already been
  // prefetched.
  prefetch();

  // Possible offset values are:
  //   0: start at the beginning
  //   1: start after .
  //   2: start after ..
  //   2+N: start after inode N

  if (off <= 0) {
    if (!list.add(".", getNodeId().get(), dtype_t::Dir, 1)) {
      return std::move(list);
    }
  }
  if (off <= 1) {
    // It's okay to query the parent without the rename lock held because, if
    // readdir is racing with rename, the results are unspecified anyway.
    // http://pubs.opengroup.org/onlinepubs/007908799/xsh/readdir.html
    auto parent = getParentRacy();
    // For the root of the mount point, just add its own inode ID as its parent.
    // FUSE seems to overwrite the parent inode number on the root dir anyway.
    auto parentNodeId = parent ? parent->getNodeId() : getNodeId();
    if (!list.add("..", parentNodeId.get(), dtype_t::Dir, 2)) {
      return std::move(list);
    }
  }

  auto dir = contents_.rlock();
  auto& entries = dir->entries;

  // Compute an index into the PathMap by InodeNumber, only including the
  // entries that are greater than the given offset.
  std::vector<std::pair<InodeNumber, size_t>> indices;
  indices.reserve(entries.size());
  size_t index = 0;
  for (auto& entry : entries) {
    auto inodeNumber = entry.second.getInodeNumber();
    if (static_cast<off_t>(inodeNumber.get() + 2) > off) {
      indices.emplace_back(entry.second.getInodeNumber(), index);
    }
    ++index;
  }
  std::make_heap(indices.begin(), indices.end(), std::greater<>{});

  // The provided DirList has limited space. Add entries until no more fit.
  while (indices.size()) {
    std::pop_heap(indices.begin(), indices.end(), std::greater<>{});
    auto& [name, entry] = entries.begin()[indices.back().second];
    indices.pop_back();

    if (!list.add(
            name.stringPiece(),
            entry.getInodeNumber().get(),
            entry.getDtype(),
            entry.getInodeNumber().get() + 2)) {
      break;
    }
  }

  return std::move(list);
}

InodeMap* TreeInode::getInodeMap() const {
  return getMount()->getInodeMap();
}

ObjectStore* TreeInode::getStore() const {
  return getMount()->getObjectStore();
}

Future<Unit> TreeInode::diff(
    const DiffContext* context,
    RelativePathPiece currentPath,
    shared_ptr<const Tree> tree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << getLogPath()
               << " cancelled due to client request no longer being active";
    return makeFuture();
  }

  InodePtr inode;
  auto gitignoreInodeFuture = Future<InodePtr>::makeEmpty();
  vector<IncompleteInodeLoad> pendingLoads;
  {
    // We have to get a write lock since we may have to load
    // the .gitignore inode, which changes the entry status
    auto contents = contents_.wlock();

    XLOG(DBG7) << "diff() on directory " << getLogPath() << " (" << getNodeId()
               << ", "
               << (contents->isMaterialized() ? "materialized"
                                              : contents->treeHash->toString())
               << ") vs " << (tree ? tree->getHash().toString() : "null tree");

    // Check to see if we can short-circuit the diff operation if we have the
    // same hash as the tree we are being compared to.
    if (!contents->isMaterialized() && tree &&
        contents->treeHash.value() == tree->getHash()) {
      // There are no changes in our tree or any children subtrees.
      return makeFuture();
    }

    // If this directory is already ignored, we don't need to bother loading its
    // .gitignore file.  Everything inside this directory must also be ignored,
    // unless it is explicitly tracked in source control.
    //
    // Explicit include rules cannot be used to unignore files inside an ignored
    // directory.
    if (isIgnored) {
      // We can pass in a null GitIgnoreStack pointer here.
      // Since the entire directory is ignored, we don't need to check ignore
      // status for any entries that aren't already tracked in source control.
      return computeDiff(
          std::move(contents),
          context,
          currentPath,
          std::move(tree),
          nullptr,
          isIgnored);
    }

    // Load the ignore rules for this directory.
    //
    // In our repositories less than .1% of directories contain a .gitignore
    // file, so we optimize for the case where a .gitignore isn't present.
    // When there is no .gitignore file we avoid acquiring and releasing the
    // contents_ lock twice, and we avoid creating a Future to load the
    // .gitignore data.
    DirEntry* gitignoreEntry = nullptr;
    auto iter = contents->entries.find(kIgnoreFilename);
    if (iter != contents->entries.end()) {
      gitignoreEntry = &iter->second;
      if (gitignoreEntry->isDirectory()) {
        // Ignore .gitignore directories
        XLOG(DBG4) << "Ignoring .gitignore directory in " << getLogPath();
        gitignoreEntry = nullptr;
      }
    }

    if (!gitignoreEntry) {
      return computeDiff(
          std::move(contents),
          context,
          currentPath,
          std::move(tree),
          make_unique<GitIgnoreStack>(parentIgnore), // empty with no rules
          isIgnored);
    }

    XLOG(DBG7) << "Loading ignore file for " << getLogPath();
    inode = gitignoreEntry->getInodePtr();
    if (!inode) {
      gitignoreInodeFuture = loadChildLocked(
          contents->entries, kIgnoreFilename, *gitignoreEntry, pendingLoads);
    }
  }

  // Finish setting up any load operations we started while holding the
  // contents_ lock above.
  for (auto& load : pendingLoads) {
    load.finish();
  }

  if (!inode) {
    return std::move(gitignoreInodeFuture)
        .thenValue([self = inodePtrFromThis(),
                    context,
                    currentPath = RelativePath{currentPath},
                    tree = std::move(tree),
                    parentIgnore,
                    isIgnored](InodePtr&& loadedInode) mutable {
          return self->loadGitIgnoreThenDiff(
              std::move(loadedInode),
              context,
              currentPath,
              std::move(tree),
              parentIgnore,
              isIgnored);
        });
  } else {
    return loadGitIgnoreThenDiff(
        std::move(inode),
        context,
        currentPath,
        std::move(tree),
        parentIgnore,
        isIgnored);
  }
}

Future<Unit> TreeInode::loadGitIgnoreThenDiff(
    InodePtr gitignoreInode,
    const DiffContext* context,
    RelativePathPiece currentPath,
    shared_ptr<const Tree> tree,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  return getMount()
      ->loadFileContents(gitignoreInode)
      .thenError([](const folly::exception_wrapper& ex) {
        XLOG(WARN) << "error reading ignore file: " << folly::exceptionStr(ex);
        return std::string{};
      })
      .thenValue([self = inodePtrFromThis(),
                  context,
                  currentPath = RelativePath{currentPath}, // deep copy
                  tree,
                  parentIgnore,
                  isIgnored](std::string&& ignoreFileContents) mutable {
        return self->computeDiff(
            self->contents_.wlock(),
            context,
            currentPath,
            std::move(tree),
            make_unique<GitIgnoreStack>(parentIgnore, ignoreFileContents),
            isIgnored);
      });
}

Future<Unit> TreeInode::computeDiff(
    folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
    const DiffContext* context,
    RelativePathPiece currentPath,
    shared_ptr<const Tree> tree,
    std::unique_ptr<GitIgnoreStack> ignore,
    bool isIgnored) {
  DCHECK(isIgnored || ignore != nullptr)
      << "the ignore stack is required if this directory is not ignored";

  // A list of entries that have been removed
  std::vector<const TreeEntry*> removedEntries;

  // A list of untracked files
  std::vector<PathComponent> untrackedFiles;
  // A list of ignored files
  std::vector<PathComponent> ignoredFiles;
  // A list of modified files
  std::vector<PathComponent> modifiedFiles;

  std::vector<std::unique_ptr<DeferredDiffEntry>> deferredEntries;
  auto self = inodePtrFromThis();

  // Grab the contents_ lock, and loop to find children that might be
  // different.  In this first pass we primarily build the list of children to
  // examine, but we wait until after we release our contents_ lock to actually
  // examine any children InodeBase objects.
  std::vector<IncompleteInodeLoad> pendingLoads;
  {
    // Move the contents lock into a variable inside this scope so it
    // will be released at the end of this scope.
    //
    // Even though diffing conceptually seems like a read-only operation, we
    // need a write lock since we may have to load child inodes, affecting
    // their entry state.
    auto contents = std::move(contentsLock);

    auto processUntracked = [&](PathComponentPiece name, DirEntry* inodeEntry) {
      bool entryIgnored = isIgnored;
      auto fileType = inodeEntry->isDirectory() ? GitIgnore::TYPE_DIR
                                                : GitIgnore::TYPE_FILE;
      auto entryPath = currentPath + name;
      if (!isIgnored) {
        auto ignoreStatus = ignore->match(entryPath, fileType);
        if (ignoreStatus == GitIgnore::HIDDEN) {
          // Completely skip over hidden entries.
          // This is used for reserved directories like .hg and .eden
          XLOG(DBG9) << "diff: hidden entry: " << entryPath;
          return;
        }
        entryIgnored = (ignoreStatus == GitIgnore::EXCLUDE);
      }

      if (inodeEntry->isDirectory()) {
        if (!entryIgnored || context->listIgnored) {
          if (auto childPtr = inodeEntry->getInodePtr()) {
            deferredEntries.emplace_back(
                DeferredDiffEntry::createUntrackedEntryFromInodeFuture(
                    context,
                    entryPath,
                    std::move(childPtr),
                    ignore.get(),
                    entryIgnored));
          } else {
            auto inodeFuture = self->loadChildLocked(
                contents->entries, name, *inodeEntry, pendingLoads);
            deferredEntries.emplace_back(
                DeferredDiffEntry::createUntrackedEntryFromInodeFuture(
                    context,
                    entryPath,
                    std::move(inodeFuture),
                    ignore.get(),
                    entryIgnored));
          }
        }
      } else {
        if (!entryIgnored) {
          XLOG(DBG8) << "diff: untracked file: " << entryPath;
          context->callback->addedFile(entryPath);
        } else if (context->listIgnored) {
          XLOG(DBG9) << "diff: ignored file: " << entryPath;
          context->callback->ignoredFile(entryPath);
        } else {
          // Don't bother reporting this ignored file since
          // listIgnored is false.
        }
      }
    };

    auto processRemoved = [&](const TreeEntry& scmEntry) {
      if (scmEntry.isTree()) {
        deferredEntries.emplace_back(DeferredDiffEntry::createRemovedEntry(
            context, currentPath + scmEntry.getName(), scmEntry));
      } else {
        XLOG(DBG5) << "diff: removed file: "
                   << currentPath + scmEntry.getName();
        context->callback->removedFile(currentPath + scmEntry.getName());
      }
    };

    auto processBothPresent = [&](const TreeEntry& scmEntry,
                                  DirEntry* inodeEntry) {
      // We only need to know the ignored status if this is a directory.
      // If this is a regular file on disk and in source control, then it
      // is always included since it is already tracked in source control.
      bool entryIgnored = isIgnored;
      auto entryPath = currentPath + scmEntry.getName();
      if (!isIgnored && (inodeEntry->isDirectory() || scmEntry.isTree())) {
        auto ignoreStatus = ignore->match(entryPath, GitIgnore::TYPE_DIR);
        if (ignoreStatus == GitIgnore::HIDDEN) {
          // This is rather unexpected.  We don't expect to find entries in
          // source control using reserved hidden names.
          // Treat this as ignored for now.
          entryIgnored = true;
        } else if (ignoreStatus == GitIgnore::EXCLUDE) {
          entryIgnored = true;
        } else {
          entryIgnored = false;
        }
      }

      if (inodeEntry->getInode()) {
        // This inode is already loaded.
        auto childInodePtr = inodeEntry->getInodePtr();
        deferredEntries.emplace_back(DeferredDiffEntry::createModifiedEntry(
            context,
            entryPath,
            scmEntry,
            std::move(childInodePtr),
            ignore.get(),
            entryIgnored));
      } else if (inodeEntry->isMaterialized()) {
        // This inode is not loaded but is materialized.
        // We'll have to load it to confirm if it is the same or different.
        auto inodeFuture = self->loadChildLocked(
            contents->entries, scmEntry.getName(), *inodeEntry, pendingLoads);
        deferredEntries.emplace_back(
            DeferredDiffEntry::createModifiedEntryFromInodeFuture(
                context,
                entryPath,
                scmEntry,
                std::move(inodeFuture),
                ignore.get(),
                entryIgnored));
      } else if (
          // Eventually the mode will come from inode metadata storage,
          // not from the directory entry.  However, any source-control-visible
          // metadata changes will cause the inode to be materialized, and
          // the previous path will be taken.
          treeEntryTypeFromMode(inodeEntry->getInitialMode()) ==
              scmEntry.getType() &&
          inodeEntry->getHash() == scmEntry.getHash()) {
        // This file or directory is unchanged.  We can skip it.
        XLOG(DBG9) << "diff: unchanged unloaded file: " << entryPath;
      } else if (inodeEntry->isDirectory()) {
        // This is a modified directory.  We have to load it then recurse
        // into it to find files with differences.
        auto inodeFuture = self->loadChildLocked(
            contents->entries, scmEntry.getName(), *inodeEntry, pendingLoads);
        deferredEntries.emplace_back(
            DeferredDiffEntry::createModifiedEntryFromInodeFuture(
                context,
                entryPath,
                scmEntry,
                std::move(inodeFuture),
                ignore.get(),
                entryIgnored));
      } else if (scmEntry.isTree()) {
        // This used to be a directory in the source control state,
        // but is now a file or symlink.  Report the new file, then add a
        // deferred entry to report the entire source control Tree as
        // removed.
        if (entryIgnored) {
          if (context->listIgnored) {
            XLOG(DBG6) << "diff: directory --> ignored file: " << entryPath;
            context->callback->ignoredFile(entryPath);
          }
        } else {
          XLOG(DBG6) << "diff: directory --> untracked file: " << entryPath;
          context->callback->addedFile(entryPath);
        }
        deferredEntries.emplace_back(DeferredDiffEntry::createRemovedEntry(
            context, entryPath, scmEntry));
      } else {
        // This file corresponds to a different blob hash, or has a
        // different mode.
        //
        // Ideally we should be able to assume that the file is
        // modified--if two blobs have different hashes we should be able
        // to assume that their contents are different.  Unfortunately this
        // is not the case for now with our mercurial blob IDs, since the
        // mercurial blob data includes the path name and past history
        // information.
        //
        // TODO: Once we build a new backing store and can replace our
        // janky hashing scheme for mercurial data, we should be able just
        // immediately assume the file is different here, without checking.
        if (treeEntryTypeFromMode(inodeEntry->getInitialMode()) !=
            scmEntry.getType()) {
          // The mode is definitely modified
          XLOG(DBG5) << "diff: file modified due to mode change: " << entryPath;
          context->callback->modifiedFile(entryPath);
        } else {
          // TODO: Hopefully at some point we will track file sizes in the
          // parent TreeInode::Entry and the TreeEntry.  Once we have file
          // sizes, we could check for differing file sizes first, and
          // avoid loading the blob if they are different.
          deferredEntries.emplace_back(DeferredDiffEntry::createModifiedEntry(
              context, entryPath, scmEntry, inodeEntry->getHash()));
        }
      }
    };

    // Walk through the source control tree entries and our inode entries to
    // look for differences.
    //
    // This code relies on the fact that the source control entries and our
    // inode entries are both sorted in the same order.
    vector<TreeEntry> emptyEntries;
    const auto& scEntries = tree ? tree->getTreeEntries() : emptyEntries;
    auto& inodeEntries = contents->entries;
    size_t scIdx = 0;
    auto inodeIter = inodeEntries.begin();
    while (true) {
      if (scIdx >= scEntries.size()) {
        if (inodeIter == inodeEntries.end()) {
          // All Done
          break;
        }

        // This entry is present locally but not in the source control tree.
        processUntracked(inodeIter->first, &inodeIter->second);
        ++inodeIter;
      } else if (inodeIter == inodeEntries.end()) {
        // This entry is present in the old tree but not the old one.
        processRemoved(scEntries[scIdx]);
        ++scIdx;
      } else if (scEntries[scIdx].getName() < inodeIter->first) {
        processRemoved(scEntries[scIdx]);
        ++scIdx;
      } else if (scEntries[scIdx].getName() > inodeIter->first) {
        processUntracked(inodeIter->first, &inodeIter->second);
        ++inodeIter;
      } else {
        const auto& scmEntry = scEntries[scIdx];
        auto* inodeEntry = &inodeIter->second;
        ++scIdx;
        ++inodeIter;
        processBothPresent(scmEntry, inodeEntry);
      }
    }
  }

  // Finish setting up any load operations we started while holding the
  // contents_ lock above.
  for (auto& load : pendingLoads) {
    load.finish();
  }

  // Now process all of the deferred work.
  vector<Future<Unit>> deferredFutures;
  for (auto& entry : deferredEntries) {
    deferredFutures.push_back(entry->run());
  }

  // Wait on all of the deferred entries to complete.
  // Note that we explicitly move-capture the deferredFutures vector into this
  // callback, to ensure that the DeferredDiffEntry objects do not get
  // destroyed before they complete.
  return folly::collectAllSemiFuture(deferredFutures)
      .toUnsafeFuture()
      .thenValue([self = std::move(self),
                  currentPath = RelativePath{std::move(currentPath)},
                  context,
                  // Capture ignore to ensure it remains valid until all of our
                  // children's diff operations complete.
                  ignore = std::move(ignore),
                  deferredJobs = std::move(deferredEntries)](
                     vector<folly::Try<Unit>> results) {
        // Call diffError() for any jobs that failed.
        for (size_t n = 0; n < results.size(); ++n) {
          auto& result = results[n];
          if (result.hasException()) {
            XLOG(WARN) << "exception processing diff for "
                       << deferredJobs[n]->getPath() << ": "
                       << folly::exceptionStr(result.exception());
            context->callback->diffError(
                deferredJobs[n]->getPath(), result.exception());
          }
        }
        // Report success here, even if some of our deferred jobs failed.
        // We will have reported those errors to the callback already, and so we
        // don't want our parent to report a new error at our path.
        return makeFuture();
      });
}

Future<Unit> TreeInode::checkout(
    CheckoutContext* ctx,
    std::shared_ptr<const Tree> fromTree,
    std::shared_ptr<const Tree> toTree) {
  XLOG(DBG4) << "checkout: starting update of " << getLogPath() << ": "
             << (fromTree ? fromTree->getHash().toString() : "<none>")
             << " --> " << (toTree ? toTree->getHash().toString() : "<none>");

  vector<unique_ptr<CheckoutAction>> actions;
  vector<IncompleteInodeLoad> pendingLoads;
  bool wasDirectoryListModified = false;

  computeCheckoutActions(
      ctx,
      fromTree.get(),
      toTree.get(),
      actions,
      pendingLoads,
      wasDirectoryListModified);

  // Wire up the callbacks for any pending inode loads we started
  for (auto& load : pendingLoads) {
    load.finish();
  }

  // Now start all of the checkout actions
  vector<Future<InvalidationRequired>> actionFutures;
  for (const auto& action : actions) {
    actionFutures.emplace_back(action->run(ctx, getStore()));
  }
  // Wait for all of the actions, and record any errors.
  return folly::collectAllSemiFuture(actionFutures)
      .toUnsafeFuture()
      .thenValue(
          [ctx,
           self = inodePtrFromThis(),
           toTree = std::move(toTree),
           actions = std::move(actions),
           wasDirectoryListModified](
              vector<folly::Try<InvalidationRequired>> actionResults) mutable {
            // Record any errors that occurred
            size_t numErrors = 0;
            for (size_t n = 0; n < actionResults.size(); ++n) {
              auto& result = actionResults[n];
              if (!result.hasException()) {
                wasDirectoryListModified |=
                    (result.value() == InvalidationRequired::Yes);
                continue;
              }
              ++numErrors;
              ctx->addError(
                  self.get(), actions[n]->getEntryName(), result.exception());
            }

            if (wasDirectoryListModified) {
              self->invalidateFuseInodeCache();
            }

            // Update our state in the overlay
            self->saveOverlayPostCheckout(ctx, toTree.get());

            XLOG(DBG4) << "checkout: finished update of " << self->getLogPath()
                       << ": " << numErrors << " errors";
          });
}

bool TreeInode::canShortCircuitCheckout(
    CheckoutContext* ctx,
    const Hash& treeHash,
    const Tree* fromTree,
    const Tree* toTree) {
  if (ctx->isDryRun()) {
    // In a dry-run update we only care about checking for conflicts
    // with the fromTree state.  Since we aren't actually performing any
    // updates we can bail out early as long as there are no conflicts.
    if (fromTree) {
      return treeHash == fromTree->getHash();
    } else {
      // There is no fromTree.  If we are already in the desired destination
      // state we don't have conflicts.  Otherwise we have to continue and
      // check for conflicts.
      return !toTree || treeHash == toTree->getHash();
    }
  }

  // For non-dry-run updates we definitely have to keep going if we aren't in
  // the desired destination state.
  if (!toTree || treeHash != toTree->getHash()) {
    return false;
  }

  // If we still here we are already in the desired destination state.
  // If there is no fromTree then the only possible conflicts are
  // UNTRACKED_ADDED conflicts, but since we are already in the desired
  // destination state these aren't really conflicts and are automatically
  // resolved.
  if (!fromTree) {
    return true;
  }

  // TODO: If we are doing a force update we should probably short circuit in
  // this case, even if there are conflicts.  For now we don't short circuit
  // just so we can report the conflicts even though we ignore them and perform
  // the update anyway.  However, none of our callers need the conflict list.
  // In the future we should probably just change the checkout API to never
  // return conflict information for force update operations.

  // Allow short circuiting if we are also the same as the fromTree state.
  return treeHash == fromTree->getHash();
}

void TreeInode::computeCheckoutActions(
    CheckoutContext* ctx,
    const Tree* fromTree,
    const Tree* toTree,
    vector<unique_ptr<CheckoutAction>>& actions,
    vector<IncompleteInodeLoad>& pendingLoads,
    bool& wasDirectoryListModified) {
  // Grab the contents_ lock for the duration of this function
  auto contents = contents_.wlock();

  // If we are the same as some known source control Tree, check to see if we
  // can quickly tell if we have nothing to do for this checkout operation and
  // can return early.
  if (contents->treeHash.has_value() &&
      canShortCircuitCheckout(
          ctx, contents->treeHash.value(), fromTree, toTree)) {
    return;
  }

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
  const auto& newEntries = toTree ? toTree->getTreeEntries() : emptyEntries;
  while (true) {
    unique_ptr<CheckoutAction> action;

    if (oldIdx >= oldEntries.size()) {
      if (newIdx >= newEntries.size()) {
        // All Done
        break;
      }

      // This entry is present in the new tree but not the old one.
      action = processCheckoutEntry(
          ctx,
          contents->entries,
          nullptr,
          &newEntries[newIdx],
          pendingLoads,
          wasDirectoryListModified);
      ++newIdx;
    } else if (newIdx >= newEntries.size()) {
      // This entry is present in the old tree but not the old one.
      action = processCheckoutEntry(
          ctx,
          contents->entries,
          &oldEntries[oldIdx],
          nullptr,
          pendingLoads,
          wasDirectoryListModified);
      ++oldIdx;
    } else if (oldEntries[oldIdx].getName() < newEntries[newIdx].getName()) {
      action = processCheckoutEntry(
          ctx,
          contents->entries,
          &oldEntries[oldIdx],
          nullptr,
          pendingLoads,
          wasDirectoryListModified);
      ++oldIdx;
    } else if (oldEntries[oldIdx].getName() > newEntries[newIdx].getName()) {
      action = processCheckoutEntry(
          ctx,
          contents->entries,
          nullptr,
          &newEntries[newIdx],
          pendingLoads,
          wasDirectoryListModified);
      ++newIdx;
    } else {
      action = processCheckoutEntry(
          ctx,
          contents->entries,
          &oldEntries[oldIdx],
          &newEntries[newIdx],
          pendingLoads,
          wasDirectoryListModified);
      ++oldIdx;
      ++newIdx;
    }

    if (action) {
      actions.push_back(std::move(action));
    }
  }
}

unique_ptr<CheckoutAction> TreeInode::processCheckoutEntry(
    CheckoutContext* ctx,
    DirContents& contents,
    const TreeEntry* oldScmEntry,
    const TreeEntry* newScmEntry,
    vector<IncompleteInodeLoad>& pendingLoads,
    bool& wasDirectoryListModified) {
  XLOG(DBG5) << "processCheckoutEntry(" << getLogPath()
             << "): " << (oldScmEntry ? oldScmEntry->toLogString() : "(null)")
             << " -> " << (newScmEntry ? newScmEntry->toLogString() : "(null)");
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
      oldScmEntry->getType() == newScmEntry->getType() &&
      oldScmEntry->getHash() == newScmEntry->getHash()) {
    // TODO: Should we perhaps fall through anyway to report conflicts for
    // locally modified files?
    return nullptr;
  }

  // Look to see if we have a child entry with this name.
  bool contentsUpdated = false;
  const auto& name =
      oldScmEntry ? oldScmEntry->getName() : newScmEntry->getName();
  auto it = contents.find(name);
  if (it == contents.end()) {
    if (!oldScmEntry) {
      // This is a new entry being added, that did not exist in the old tree
      // and does not currently exist in the filesystem.  Go ahead and add it
      // now.
      if (!ctx->isDryRun()) {
        contents.emplace(
            newScmEntry->getName(),
            modeFromTreeEntryType(newScmEntry->getType()),
            getOverlay()->allocateInodeNumber(),
            newScmEntry->getHash());
        contentsUpdated = true;
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
        DCHECK(!ctx->isDryRun());
        contents.emplace(
            newScmEntry->getName(),
            modeFromTreeEntryType(newScmEntry->getType()),
            getOverlay()->allocateInodeNumber(),
            newScmEntry->getHash());
        contentsUpdated = true;
      }
    }

    if (contentsUpdated) {
      // Contents have changed and they need to be written out to the
      // overlay.  We should not do that here since this code runs per
      // entry. Today this is reconciled in saveOverlayPostCheckout()
      // after this inode processes all of its checkout actions. But we
      // do want to invalidate the kernel's dcache and inode caches.
      wasDirectoryListModified = true;
      invalidateFuseEntryCache(name);
    }

    // Nothing else to do when there is no local inode.
    return nullptr;
  }

  auto& entry = it->second;
  if (auto childPtr = entry.getInodePtr()) {
    // If the inode is already loaded, create a CheckoutAction to process it
    return make_unique<CheckoutAction>(
        ctx, oldScmEntry, newScmEntry, std::move(childPtr));
  }

  // If true, preserve inode numbers for files that have been accessed and
  // still remain when a tree transitions from A -> B.  This is really expensive
  // because it means we must load TreeInodes for all trees that have ever
  // allocated inode numbers.
  constexpr bool kPreciseInodeNumberMemory = false;

  // If a load for this entry is in progress, then we have to wait for the
  // load to finish.  Loading the inode ourself will wait for the existing
  // attempt to finish.
  // We also have to load the inode if it is materialized so we can
  // check its contents to see if there are conflicts or not.
  if (entry.isMaterialized() ||
      getInodeMap()->isInodeRemembered(entry.getInodeNumber()) ||
      (kPreciseInodeNumberMemory && entry.isDirectory() &&
       getOverlay()->hasOverlayData(entry.getInodeNumber()))) {
    XLOG(DBG6) << "must load child: inode=" << getNodeId() << " child=" << name;
    // This child is potentially modified (or has saved state that must be
    // updated), but is not currently loaded. Start loading it and create a
    // CheckoutAction to process it once it is loaded.
    auto inodeFuture = loadChildLocked(contents, name, entry, pendingLoads);
    return make_unique<CheckoutAction>(
        ctx, oldScmEntry, newScmEntry, std::move(inodeFuture));
  } else {
    XLOG(DBG6) << "not loading child: inode=" << getNodeId()
               << " child=" << name;
  }

  // Check for conflicts
  auto conflictType = ConflictType::ERROR;
  if (!oldScmEntry) {
    conflictType = ConflictType::UNTRACKED_ADDED;
  } else if (entry.getHash() != oldScmEntry->getHash()) {
    conflictType = ConflictType::MODIFIED_MODIFIED;
  }
  if (conflictType != ConflictType::ERROR) {
    // If this is a directory we unfortunately have to load it and recurse into
    // it just so we can accurately report the list of files with conflicts.
    if (entry.isDirectory()) {
      auto inodeFuture = loadChildLocked(contents, name, entry, pendingLoads);
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
  if (ctx->isDryRun()) {
    return nullptr;
  }

  auto oldEntryInodeNumber = entry.getInodeNumber();

  // Update the entry
  if (!newScmEntry) {
    // TODO: remove entry.getInodeNumber() from both the overlay and the
    // InodeTable.  Or at least verify that it's already done in a test.
    //
    // This logic could potentially be unified with TreeInode::tryRemoveChild
    // and TreeInode::checkoutUpdateEntry.
    contents.erase(it);
  } else {
    entry = DirEntry{modeFromTreeEntryType(newScmEntry->getType()),
                     getOverlay()->allocateInodeNumber(),
                     newScmEntry->getHash()};
  }

  wasDirectoryListModified = true;

  // Contents have changed and the entry is not materialized, but we may have
  // allocated and remembered inode numbers for this tree.  It's much faster to
  // simply forget the inode numbers we allocated here -- if we were a real
  // filesystem, it's as if the entire subtree got deleted and checked out
  // from scratch.  (Note: if anything uses Watchman and cares precisely about
  // inode numbers, it could miss changes.)
  if (!kPreciseInodeNumberMemory && entry.isDirectory()) {
    XLOG(DBG5) << "recursively removing overlay data for "
               << oldEntryInodeNumber << "(" << getLogPath() << " / " << name
               << ")";
    getOverlay()->recursivelyRemoveOverlayData(oldEntryInodeNumber);
  }

  // TODO: contents have changed: we probably should propagate
  // this information up to our caller so it can mark us
  // materialized if necessary.

  // We removed or replaced an entry - invalidate it.
  auto* fuseChannel = getMount()->getFuseChannel();
  if (fuseChannel) {
    fuseChannel->invalidateEntry(getNodeId(), name);
  }

  return nullptr;
}

Future<InvalidationRequired> TreeInode::checkoutUpdateEntry(
    CheckoutContext* ctx,
    PathComponentPiece name,
    InodePtr inode,
    std::shared_ptr<const Tree> oldTree,
    std::shared_ptr<const Tree> newTree,
    const std::optional<TreeEntry>& newScmEntry) {
  auto treeInode = inode.asTreePtrOrNull();
  if (!treeInode) {
    // If the target of the update is not a directory, then we know we do not
    // need to recurse into it, looking for more conflicts, so we can exit here.
    if (ctx->isDryRun()) {
      return InvalidationRequired::No;
    }

    {
      std::unique_ptr<InodeBase> deletedInode;
      auto contents = contents_.wlock();

      // The CheckoutContext should be holding the rename lock, so the entry
      // at this name should still be the specified inode.
      auto it = contents->entries.find(name);
      if (it == contents->entries.end()) {
        return EDEN_BUG_FUTURE(InvalidationRequired)
            << "entry removed while holding rename lock during checkout: "
            << inode->getLogPath();
      }
      if (it->second.getInode() != inode.get()) {
        return EDEN_BUG_FUTURE(InvalidationRequired)
            << "entry changed while holding rename lock during checkout: "
            << inode->getLogPath();
      }

      // This is a file, so we can simply unlink it, and replace/remove the
      // entry as desired.
      deletedInode = inode->markUnlinked(this, name, ctx->renameLock());
      if (newScmEntry) {
        DCHECK_EQ(newScmEntry->getName(), name);
        it->second = DirEntry(
            modeFromTreeEntryType(newScmEntry->getType()),
            getOverlay()->allocateInodeNumber(),
            newScmEntry->getHash());
      } else {
        contents->entries.erase(it);
      }
    }

    // Tell FUSE to invalidate its cache for this entry.
    invalidateFuseEntryCache(name);

    // We don't save our own overlay data right now:
    // we'll wait to do that until the checkout operation finishes touching all
    // of our children in checkout().
    return InvalidationRequired::Yes;
  }

  // If we are going from a directory to a directory, all we need to do
  // is call checkout().
  if (newTree) {
    // TODO: Also apply permissions changes to the entry.

    CHECK(newScmEntry.has_value());
    CHECK(newScmEntry->isTree());
    return treeInode->checkout(ctx, std::move(oldTree), std::move(newTree))
        .thenValue([](folly::Unit) { return InvalidationRequired::No; });
  }

  if (ctx->isDryRun()) {
    // TODO(mbolin): As it stands, if this is a dry run, we will not report a
    // DIRECTORY_NOT_EMPTY conflict if it exists. We need to do further
    // investigation to determine whether this is acceptible behavior.
    // Currently, the Hg extension ignores DIRECTORY_NOT_EMPTY conflicts, but
    // that may not be the right thing to do.
    return InvalidationRequired::No;
  }

  // We need to remove this directory (and possibly replace it with a file).
  // First we have to recursively unlink everything inside the directory.
  // Fortunately, calling checkout() with an empty destination tree does
  // exactly what we want.  checkout() will even remove the directory before it
  // returns if the directory is empty.
  return treeInode->checkout(ctx, std::move(oldTree), nullptr)
      .thenValue(
          [ctx,
           name = PathComponent{name},
           parentInode = inodePtrFromThis(),
           treeInode,
           newScmEntry](auto &&) -> folly::Future<InvalidationRequired> {
            // Make sure the treeInode was completely removed by the checkout.
            // If there were still untracked files inside of it, it won't have
            // been deleted, and we have a conflict that we cannot resolve.
            if (!treeInode->isUnlinked()) {
              ctx->addConflict(
                  ConflictType::DIRECTORY_NOT_EMPTY, treeInode.get());
              return InvalidationRequired::No;
            }

            if (!newScmEntry) {
              // checkout() will invalidate the parent inode if it removes a
              // child because it becomes an empty tree, so we don't need to
              // invalidate here.
              return InvalidationRequired::No;
            }

            // Add the new entry
            bool inserted;
            {
              auto contents = parentInode->contents_.wlock();
              DCHECK(!newScmEntry->isTree());
              auto ret = contents->entries.emplace(
                  name,
                  modeFromTreeEntryType(newScmEntry->getType()),
                  parentInode->getOverlay()->allocateInodeNumber(),
                  newScmEntry->getHash());
              inserted = ret.second;
            }

            // This code is running asynchronously during checkout, so
            // flush the readdir cache right here.
            parentInode->invalidateFuseInodeCache();

            if (!inserted) {
              // Hmm.  Someone else already created a new entry in this location
              // before we had a chance to add our new entry.  We don't block
              // new file or directory creations during a checkout operation, so
              // this is possible.  Just report an error in this case.
              ctx->addError(
                  parentInode.get(),
                  name,
                  InodeError(
                      EEXIST,
                      parentInode,
                      name,
                      "new file created with this name while checkout operation "
                      "was in progress"));
            }

            // Return false because the code above has already invalidated
            // this inode's readdir cache, so we don't technically need to
            // do it again unless something else modifies the contents.
            return InvalidationRequired::No;
          });
}

void TreeInode::invalidateFuseInodeCache() {
  if (auto* fuseChannel = getMount()->getFuseChannel()) {
    // FUSE_NOTIFY_INVAL_ENTRY is the appropriate invalidation function
    // when an entry is removed or modified. But when new entries are
    // added, the inode itself must be invalidated.
    fuseChannel->invalidateInode(getNodeId(), 0, 0);
  }
}

void TreeInode::invalidateFuseInodeCacheIfRequired() {
  if (RequestData::isFuseRequest()) {
    // no need to flush the cache if we are inside a FUSE request handler
    return;
  }
  invalidateFuseInodeCache();
}

void TreeInode::invalidateFuseEntryCache(PathComponentPiece name) {
  if (auto* fuseChannel = getMount()->getFuseChannel()) {
    fuseChannel->invalidateEntry(getNodeId(), name);
  }
}

void TreeInode::invalidateFuseEntryCacheIfRequired(PathComponentPiece name) {
  if (RequestData::isFuseRequest()) {
    // no need to flush the cache if we are inside a FUSE request handler
    return;
  }
  invalidateFuseEntryCache(name);
}

void TreeInode::saveOverlayPostCheckout(
    CheckoutContext* ctx,
    const Tree* tree) {
  if (ctx->isDryRun()) {
    // If this is a dry run, then we do not want to update the parents or make
    // any sort of unnecessary writes to the overlay, so we bail out.
    return;
  }

  bool isMaterialized;
  bool stateChanged;
  bool deleteSelf;
  {
    auto contents = contents_.wlock();

    // Check to see if we need to be materialized or not.
    //
    // If we can confirm that we are identical to the source control Tree we do
    // not need to be materialized.
    auto tryToDematerialize = [&]() -> std::optional<Hash> {
      // If the new tree does not exist in source control, we must be
      // materialized, since there is no source control Tree to refer to.
      // (If we are empty in this case we will set deleteSelf and try to remove
      // ourself entirely.)
      if (!tree) {
        return std::nullopt;
      }

      const auto& scmEntries = tree->getTreeEntries();
      // If we have a different number of entries we must be different from the
      // Tree, and therefore must be materialized.
      if (scmEntries.size() != contents->entries.size()) {
        return std::nullopt;
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
        if (inodeIter->second.isMaterialized()) {
          return std::nullopt;
        }

        // If the child is not materialized, it is the same as some source
        // control object.  However, if it isn't the same as the object in our
        // Tree, we have to materialize ourself.
        if (inodeIter->second.getHash() != scmIter->getHash()) {
          return std::nullopt;
        }
      }

      // If we're still here we are identical to the source control Tree.
      // We can be dematerialized and marked identical to the input Tree.
      return tree->getHash();
    };

    // If we are now empty as a result of the checkout we can remove ourself
    // entirely.  For now we only delete ourself if this directory doesn't
    // exist in source control either.
    deleteSelf = (!tree && contents->entries.empty());

    auto oldHash = contents->treeHash;
    contents->treeHash = tryToDematerialize();
    isMaterialized = contents->isMaterialized();
    stateChanged = (oldHash != contents->treeHash);

    XLOG(DBG4) << "saveOverlayPostCheckout(" << getLogPath() << ", " << tree
               << "): deleteSelf=" << deleteSelf << ", oldHash="
               << (oldHash ? oldHash.value().toString() : "none") << " newHash="
               << (contents->treeHash ? contents->treeHash.value().toString()
                                      : "none")
               << " isMaterialized=" << isMaterialized;

    // Update the overlay to include the new entries, even if dematerialized.
    saveOverlayDir(contents->entries);
  }

  if (deleteSelf) {
    // If we should be removed entirely, delete ourself.
    if (checkoutTryRemoveEmptyDir(ctx)) {
      return;
    }

    // We failed to remove ourself.  The most likely reason is that someone
    // created a new entry inside this directory between when we set deleteSelf
    // above and when we attempted to remove ourself.  Fall through and perform
    // the normal materialization state update in this case.
  }

  if (stateChanged) {
    // If our state changed, tell our parent.
    //
    // TODO: Currently we end up writing out overlay data for TreeInodes pretty
    // often during the checkout process.  Each time a child entry is processed
    // we will likely end up rewriting data for it's parent TreeInode, and then
    // once all children are processed we do another pass through here in
    // saveOverlayPostCheckout() and possibly write it out again.
    //
    // It would be nicer if we could only save the data for each TreeInode
    // once.  The downside of this is that the on-disk overlay state would be
    // potentially inconsistent until the checkout completes.  There may be
    // periods of time where a parent directory says the child is materialized
    // when the child has decided to be dematerialized.  This would cause
    // problems when we tried to load the overlay data later.  If we update the
    // code to be able to handle this somehow then maybe we could avoid doing
    // all of the intermediate updates to the parent as we process each child
    // entry.
    auto loc = getLocationInfo(ctx->renameLock());
    if (loc.parent && !loc.unlinked) {
      if (isMaterialized) {
        loc.parent->childMaterialized(ctx->renameLock(), loc.name);
      } else {
        loc.parent->childDematerialized(
            ctx->renameLock(), loc.name, tree->getHash());
      }
    }
  }
}

bool TreeInode::checkoutTryRemoveEmptyDir(CheckoutContext* ctx) {
  auto location = getLocationInfo(ctx->renameLock());
  DCHECK(!location.unlinked);
  if (!location.parent) {
    // We can't ever remove the root directory.
    return false;
  }

  bool flushKernelCache = true;
  auto errnoValue = location.parent->tryRemoveChild(
      ctx->renameLock(), location.name, inodePtrFromThis(), flushKernelCache);
  return (errnoValue == 0);
}

folly::Future<InodePtr> TreeInode::loadChildLocked(
    DirContents& /* contents */,
    PathComponentPiece name,
    DirEntry& entry,
    std::vector<IncompleteInodeLoad>& pendingLoads) {
  DCHECK(!entry.getInode());

  folly::Promise<InodePtr> promise;
  auto future = promise.getFuture();
  auto childNumber = entry.getInodeNumber();
  bool startLoad = getInodeMap()->shouldLoadChild(
      this, name, childNumber, std::move(promise));
  if (startLoad) {
    auto loadFuture = startLoadingInodeNoThrow(entry, name);
    pendingLoads.emplace_back(
        this, std::move(loadFuture), name, entry.getInodeNumber());
  }

  return future;
}

namespace {

/**
 * WARNING: predicate is called while the InodeMap and TreeInode contents locks
 * are held.
 */
template <typename Recurse, typename Predicate>
size_t unloadChildrenIf(
    TreeInode* const self,
    InodeMap* const inodeMap,
    std::vector<TreeInodePtr>& treeChildren,
    Recurse&& recurse,
    Predicate&& predicate) {
  size_t unloadCount = 0;

  // Recurse into children here. Children hold strong references to their parent
  // trees, so unloading children can cause the parent to become unreferenced.
  for (auto& child : treeChildren) {
    unloadCount += recurse(*child);
  }

  // Release the treeChildren refcounts.
  treeChildren.clear();

  // Unload children whose reference count is zero.
  std::vector<unique_ptr<InodeBase>> toDelete;
  {
    auto contents = self->getContents().wlock();
    auto inodeMapLock = inodeMap->lockForUnload();

    for (auto& entry : contents->entries) {
      auto* entryInode = entry.second.getInode();
      if (!entryInode) {
        continue;
      }

      // Check isPtrAcquireCountZero() first. It's a single load instruction on
      // x86 and if the predicate calls getFuseRefcount(), it will assert if
      // isPtrAcquireCountZero() is false.
      if (entryInode->isPtrAcquireCountZero() && predicate(entryInode)) {
        // If it's a tree and it has a loaded child, its refcount will never be
        // zero because the child holds a reference to its parent.

        // Allocate space in the vector. This can throw std::bad_alloc.
        toDelete.emplace_back();

        // Forget other references to this inode.
        (void)entry.second.clearInode(); // clearInode will not throw.
        inodeMap->unloadInode(
            entryInode, self, entry.first, false, inodeMapLock);

        // If unloadInode threw, we'll leak the entryInode, but it's no big
        // deal. This assignment cannot throw.
        toDelete.back() = unique_ptr<InodeBase>{entryInode};
      }
    }
  }

  unloadCount += toDelete.size();
  // Outside of the locks, deallocate all of the inodes scheduled to be deleted.
  toDelete.clear();

  return unloadCount;
}

std::vector<TreeInodePtr> getTreeChildren(TreeInode* self) {
  std::vector<TreeInodePtr> treeChildren;
  {
    auto contents = self->getContents().rlock();
    for (auto& entry : contents->entries) {
      if (!entry.second.getInode()) {
        continue;
      }

      // This has the side effect of incrementing the reference counts of all
      // of the children. When that goes back to zero,
      // InodeMap::onInodeUnreferenced will be called on the entry.
      if (auto asTree = entry.second.asTreePtrOrNull()) {
        treeChildren.emplace_back(std::move(asTree));
      }
    }
  }
  return treeChildren;
}

} // namespace

size_t TreeInode::unloadChildrenNow() {
  auto treeChildren = getTreeChildren(this);
  return unloadChildrenIf(
      this,
      getInodeMap(),
      treeChildren,
      [](TreeInode& child) { return child.unloadChildrenNow(); },
      [](InodeBase*) { return true; });
}

size_t TreeInode::unloadChildrenUnreferencedByFuse() {
  auto treeChildren = getTreeChildren(this);
  return unloadChildrenIf(
      this,
      getInodeMap(),
      treeChildren,
      [](TreeInode& child) { return child.unloadChildrenUnreferencedByFuse(); },
      [](InodeBase* child) { return child->getFuseRefcount() == 0; });
}

size_t TreeInode::unloadChildrenLastAccessedBefore(const timespec& cutoff) {
  // Unloading children by criteria is a bit of an intricate operation. The
  // InodeMap and tree's contents lock must be held simultaneously when
  // checking if an inode's refcount is zero. But the child's lock cannot be
  // acquired after the InodeMap's lock is.
  //
  // Yet the child's lock must be acquired to read the atime of an inode.
  //
  // So the strategy is to acquire a set of strong InodePtrs while the
  // parent's contents lock is held. Then check atime with those strong
  // pointers, remembering which InodeNumbers we intend to unload.
  //
  // Then reacquire the parent's contents lock and the inodemap lock and
  // determine which inodes can be deleted.

  // Get the list of inodes in the directory by holding contents lock.
  // TODO: Better yet, this shouldn't use atime at all and instead keep an
  // internal system_clock::time_point in InodeBase that updates upon any
  // interesting access.
  std::vector<FileInodePtr> fileChildren;
  std::vector<TreeInodePtr> treeChildren;
  {
    auto contents = contents_.rlock();
    for (auto& entry : contents->entries) {
      if (!entry.second.getInode()) {
        continue;
      }

      // This has the side effect of incrementing the reference counts of all
      // of the children. When that goes back to zero,
      // InodeMap::onInodeUnreferenced will be called on the entry.
      if (auto asFile = entry.second.asFilePtrOrNull()) {
        fileChildren.emplace_back(std::move(asFile));
      } else if (auto asTree = entry.second.asTreePtrOrNull()) {
        treeChildren.emplace_back(std::move(asTree));
      } else {
        EDEN_BUG() << "entry " << entry.first << " was neither a tree nor file";
      }
    }
  }

  // Now that the parent's lock is released, filter the inodes by age (i.e.
  // atime). Hold InodeNumbers because all we need to check is the identity of
  // the child's inode. This might need to be rethought when we support hard
  // links.
  std::unordered_set<InodeNumber> toUnload;

  // Is atime the right thing to check here?  If a read is served from
  // the kernel's cache, the cached atime is updated, but FUSE does not
  // tell us.  That said, if we update atime whenever FUSE forwards a
  // read request on to Eden, then atime ought to be a suitable proxy
  // for whether it's a good idea to unload the inode or not.
  //
  // https://sourceforge.net/p/fuse/mailman/message/34448996/
  auto shouldUnload = [&](const auto& inode) {
    return inode->getMetadata().timestamps.atime < cutoff;
  };

  for (const auto& inode : fileChildren) {
    if (shouldUnload(inode)) {
      toUnload.insert(inode->getNodeId());
    }
  }
  for (const auto& inode : treeChildren) {
    if (shouldUnload(inode)) {
      toUnload.insert(inode->getNodeId());
    }
  }

  // We no longer need pointers to the child inodes - release them. Beware that
  // this may deallocate inode instances for the children and clear them from
  // InodeMap and contents table as a natural side effect of their refcounts
  // going to zero.
  //
  // unloadChildrenIf below will clear treeChildren.
  fileChildren.clear();

  return unloadChildrenIf(
      this,
      getInodeMap(),
      treeChildren,
      [&](TreeInode& child) {
        return child.unloadChildrenLastAccessedBefore(cutoff);
      },
      [&](InodeBase* child) {
        return toUnload.count(child->getNodeId()) != 0;
      });
}

void TreeInode::getDebugStatus(vector<TreeInodeDebugInfo>& results) const {
  TreeInodeDebugInfo info;
  info.inodeNumber = getNodeId().get();
  info.refcount = debugGetFuseRefcount();

  auto myPath = getPath();
  if (myPath.has_value()) {
    info.path = myPath.value().stringPiece().str();
  }

  vector<std::pair<PathComponent, InodePtr>> childInodes;
  {
    auto contents = contents_.rlock();

    info.materialized = contents->isMaterialized();
    info.treeHash = thriftHash(contents->treeHash);

    for (const auto& entry : contents->entries) {
      if (entry.second.getInode()) {
        // A child inode exists, so just grab an InodePtr and add it to the
        // childInodes list.  We will process all loaded children after
        // releasing our own contents_ lock (since we need to grab each child
        // Inode's own lock to get its data).
        childInodes.emplace_back(entry.first, entry.second.getInodePtr());
      } else {
        // We can store data about unloaded entries immediately, since we have
        // the authoritative data ourself, and don't need to ask a separate
        // InodeBase object.
        info.entries.emplace_back();
        auto& infoEntry = info.entries.back();
        auto& inodeEntry = entry.second;
        infoEntry.name = entry.first.stringPiece().str();
        infoEntry.inodeNumber = inodeEntry.getInodeNumber().get();
        infoEntry.mode = inodeEntry.getInitialMode();
        infoEntry.loaded = false;
        infoEntry.materialized = inodeEntry.isMaterialized();
        if (!infoEntry.materialized) {
          infoEntry.hash = thriftHash(inodeEntry.getHash());
        }
      }
    }
  }

  std::vector<folly::Future<std::pair<size_t, uint64_t>>> futures;
  for (const auto& childData : childInodes) {
    info.entries.emplace_back();
    auto& infoEntry = info.entries.back();
    infoEntry.name = childData.first.stringPiece().str();
    infoEntry.inodeNumber = childData.second->getNodeId().get();
    infoEntry.loaded = true;

    auto childTree = childData.second.asTreePtrOrNull();
    if (childTree) {
      // The child will also store its own data when we recurse, but go ahead
      // and grab the materialization and status info now.
      {
        auto childContents = childTree->contents_.rlock();
        infoEntry.materialized = !childContents->treeHash.has_value();
        infoEntry.hash = thriftHash(childContents->treeHash);
        // TODO: We don't currently store mode data for TreeInodes.  We should.
        infoEntry.mode = (S_IFDIR | 0755);
      }
    } else {
      auto childFile = childData.second.asFilePtr();

      infoEntry.mode = childFile->getMode();
      auto blobHash = childFile->getBlobHash();
      infoEntry.materialized = !blobHash.has_value();
      infoEntry.hash = thriftHash(blobHash);
      futures.push_back(
          childFile->stat().thenValue([i = info.entries.size() - 1](auto st) {
            auto fileSize = st.st_size;
            return std::make_pair(i, fileSize);
          }));
    }
  }
  auto fileSizeMappings = folly::collectAll(futures).get();
  for (const auto& future : fileSizeMappings) {
    auto [i, fileSize] = future.value();

    auto& infoEntry = info.entries[i];
    // We must use set_size here because size is
    // optional and if it is set directly then it will
    // not get serialized correctly.
    infoEntry.set_fileSize(fileSize);
  }
  results.push_back(info);

  // Recurse into all children directories after we finish building our own
  // results.  We do this separately from the loop above just to order the
  // results nicely: parents appear before their children, and children
  // are sorted alphabetically (since contents_.entries are sorted).
  for (const auto& childData : childInodes) {
    auto childTree = childData.second.asTreePtrOrNull();
    if (childTree) {
      childTree->getDebugStatus(results);
    }
  }
}

InodeMetadata TreeInode::getMetadata() const {
  auto lock = contents_.rlock();
  return getMetadataLocked(lock->entries);
}

void TreeInode::updateAtime() {
  auto lock = contents_.wlock();
  InodeBaseMetadata::updateAtimeLocked(lock->entries);
}

InodeMetadata TreeInode::getMetadataLocked(const DirContents&) const {
  return getMount()->getInodeMetadataTable()->getOrThrow(getNodeId());
}

void TreeInode::prefetch() {
  bool expected = false;
  if (!prefetched_.compare_exchange_strong(expected, true)) {
    return;
  }
  auto prefetchLease = getMount()->tryStartTreePrefetch(inodePtrFromThis());
  if (!prefetchLease) {
    XLOG(DBG3) << "skipping prefetch for " << getLogPath()
               << ": too many prefetches already in progress";
    prefetched_.store(false);
    return;
  }
  XLOG(DBG4) << "starting prefetch for " << getLogPath();

  folly::via(
      getMount()->getThreadPool().get(),
      [lease = std::move(*prefetchLease)]() mutable {
        // prefetch() is called by readdir, under the assumption that a series
        // of stat calls on its entries will follow. (e.g. `ls -l` or `find
        // -ls`). To optimize that common situation, load trees and blob
        // metadata in parallel here.

        std::vector<IncompleteInodeLoad> pendingLoads;
        std::vector<Future<Unit>> inodeFutures;

        {
          auto contents = lease.getTreeInode()->contents_.wlock();

          for (auto& [name, entry] : contents->entries) {
            if (entry.getInode()) {
              // Already loaded
              continue;
            }

            // Userspace will commonly issue a readdir() followed by a series of
            // stat()s. In FUSE, that translates into readdir() and then
            // lookup(), which returns the same information as a stat(),
            // including the number of directory entries or number of bytes in a
            // file. Perform those operations here by loading inodes, trees, and
            // blob sizes.
            inodeFutures.emplace_back(
                lease.getTreeInode()
                    ->loadChildLocked(
                        contents->entries, name, entry, pendingLoads)
                    .thenValue([](InodePtr inode) { return inode->getattr(); })
                    .unit());
          }
        }

        // Hook up the pending load futures to properly complete the loading
        // process then the futures are ready.  We can only do this after
        // releasing the contents_ lock.
        for (auto& load : pendingLoads) {
          load.finish();
        }

        return folly::collectAll(inodeFutures)
            .thenTry([lease = std::move(lease)](auto&&) {
              XLOG(DBG4) << "finished prefetch for "
                         << lease.getTreeInode()->getLogPath();
            });
      });
}

folly::Future<Dispatcher::Attr> TreeInode::setattr(
    const fuse_setattr_in& attr) {
  materialize();
  Dispatcher::Attr result(getMount()->initStatData());

  // We do not have size field for directories and currently TreeInode does not
  // have any field like FileInode::state_::mode to set the mode. May be in the
  // future if needed we can add a mode Field to TreeInode::contents_ but for
  // now we are simply setting the mode to (S_IFDIR | 0755).

  // Set InodeNumber, timeStamps, mode in the result.
  result.st.st_ino = getNodeId().get();
  auto contents = contents_.wlock();
  auto metadata = getMount()->getInodeMetadataTable()->modifyOrThrow(
      getNodeId(),
      [&](auto& metadata) { metadata.updateFromAttr(getClock(), attr); });
  metadata.applyToStat(result.st);

  // Update Journal
  updateJournal();
  return result;
}

folly::Future<std::vector<std::string>> TreeInode::listxattr() {
  return std::vector<std::string>{};
}
folly::Future<std::string> TreeInode::getxattr(folly::StringPiece /*name*/) {
  return makeFuture<std::string>(InodeError(kENOATTR, inodePtrFromThis()));
}

} // namespace eden
} // namespace facebook
