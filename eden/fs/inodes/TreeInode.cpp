/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
#include <sys/stat.h>
#include <vector>

#include "eden/common/utils/Synchronized.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/fuse/DirList.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/CheckoutAction.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DeferredDiffEntry.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/OverlayFile.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreePrefetchLease.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/prjfs/Enumerator.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/store/DiffCallback.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/telemetry/Tracing.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/CaseSensitivity.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/ImmediateFuture.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/SystemError.h"
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

namespace facebook::eden {

namespace {
static constexpr PathComponentPiece kIgnoreFilename{".gitignore"};

/**
 * For case insensitive system, we need to use the casing of the file as
 * present in the SCM rather than the one used for lookup.
 */
static inline PathComponent copyCanonicalInodeName(
    const DirContents::const_iterator& iter) {
  return PathComponent(iter->first);
}
} // namespace

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
    std::optional<ObjectId> treeHash)
    : Base(ino, initialMode, initialTimestamps, parent, name),
      contents_(folly::in_place, std::move(dir), std::move(treeHash)) {
  XDCHECK_NE(ino, kRootNodeId);
}

TreeInode::TreeInode(EdenMount* mount, std::shared_ptr<const Tree>&& tree)
    : TreeInode(
          mount,
          saveDirFromTree(kRootNodeId, tree.get(), mount),
          tree->getHash()) {}

TreeInode::TreeInode(
    EdenMount* mount,
    DirContents&& dir,
    std::optional<ObjectId> treeHash)
    : Base(mount), contents_(folly::in_place, std::move(dir), treeHash) {}

TreeInode::~TreeInode() {}

ImmediateFuture<struct stat> TreeInode::stat(
    const ObjectFetchContextPtr& /*context*/) {
  auto st = getMount()->initStatData();
  st.st_ino = folly::to_narrow(getNodeId().get());
  auto contents = contents_.rlock();

#ifndef _WIN32
  getMetadataLocked(contents->entries).applyToStat(st);
#endif

  // For directories, nlink is the number of entries including the
  // "." and ".." links.
#ifndef _WIN32
  st.st_nlink = contents->entries.size() + 2;
#else
  st.st_nlink =
      folly::to_narrow(folly::to_signed(contents->entries.size() + 2));
#endif

  return st;
}

std::optional<ImmediateFuture<VirtualInode>> TreeInode::rlockGetOrFindChild(
    const TreeInodeState& contents,
    PathComponentPiece name,
    const ObjectFetchContextPtr& context,
    bool loadInodes) {
  // Check if the child is already loaded and return it if so
  auto iter = contents.entries.find(name);
  if (iter == contents.entries.end()) {
    XLOG(DBG7) << "attempted to load non-existent entry \"" << name << "\" in "
               << getLogPath();
    return std::make_optional(
        ImmediateFuture<VirtualInode>{folly::Try<VirtualInode>{
            InodeError(ENOENT, inodePtrFromThis(), name)}});
  }

  // Check to see if the entry is already loaded
  auto& entry = iter->second;
  if (auto inodePtr = entry.getInodePtr()) {
    return VirtualInode{std::move(inodePtr)};
  }

  // The node is not loaded. If the caller requires that we load
  // Inodes, or the entry is materialized, go on and load the inode
  // by returning std::nullopt here.
  if (loadInodes || entry.isMaterialized()) {
    return std::nullopt;
  }

  // Note that a child's inode may be currently loading. If it's
  // currently being loaded there's no chance it's been
  // modified/materialized yet (it has to have been loaded prior),
  // so it's safe here to ignore the loading inode and instead
  // query the object store for information about the path.
  auto hash = entry.getHash();
  if (entry.isDirectory()) {
    // This is a directory, always get the tree corresponding to
    // the hash
    return getObjectStore()
        .getTree(hash, context)
        .thenValue([mode = entry.getInitialMode()](
                       std::shared_ptr<const Tree>&& tree) {
          return VirtualInode(std::move(tree), mode);
        });
  }
  // This is a file, return the DirEntry if this was the last
  // path component. Note that because the entry is not loaded and
  // is not materialized, it's guaranteed to have a hash set, and
  // the constructor of UnmaterializedUnloadedBlobDirEntry can be
  // called safely.
  return VirtualInode{UnmaterializedUnloadedBlobDirEntry(entry)};
}

std::pair<folly::SemiFuture<InodePtr>, TreeInode::LoadChildCleanUp>
TreeInode::loadChild(
    folly::Synchronized<TreeInodeState>::LockedPtr& contents,
    PathComponentPiece name,
    const ObjectFetchContextPtr& context) {
  auto inodeLoadFuture = Future<unique_ptr<InodeBase>>::makeEmpty();
  InodePtr childInodePtr;
  InodeMap::PromiseVector promises;

  // The entry is not loaded yet.  Ask the InodeMap about the
  // entry. The InodeMap will tell us if this inode is already in
  // the process of being loaded, or if we need to start loading it
  // now.
  auto iter = contents->entries.find(name);
  auto inodeName = copyCanonicalInodeName(iter);
  name = inodeName.piece();
  auto& entry = iter->second;
  folly::Promise<InodePtr> promise;
  auto returnFuture = promise.getSemiFuture();
  auto childNumber = entry.getInodeNumber();
  bool startLoad = getInodeMap()->startLoadingChildIfNotLoading(
      this, name, childNumber, entry.getInitialMode(), std::move(promise));
  if (startLoad) {
    // The inode is not already being loaded.  We have to start
    // loading it now.
    auto loadFuture = startLoadingInodeNoThrow(entry, name, context);
    if (loadFuture.isReady() && loadFuture.hasValue()) {
      // If we finished loading the inode immediately, just call
      // InodeMap::inodeLoadComplete() now, since we still have the
      // data_ lock.
      auto childInode = std::move(loadFuture).get();
      entry.setInode(childInode.get());
      promises = getInodeMap()->inodeLoadComplete(childInode.get());
      childInodePtr = InodePtr::takeOwnership(std::move(childInode));
    } else {
      inodeLoadFuture = std::move(loadFuture);
    }
  }
  return std::make_pair(
      std::move(returnFuture),
      LoadChildCleanUp{
          std::move(inodeLoadFuture),
          std::move(promises),
          std::move(childNumber),
          std::move(childInodePtr),
      });
}

void TreeInode::loadChildCleanUp(
    PathComponentPiece name,
    TreeInode::LoadChildCleanUp result) {
  if (result.inodeLoadFuture.valid()) {
    registerInodeLoadComplete(result.inodeLoadFuture, name, result.childNumber);
  } else {
    for (auto& promise : result.promises) {
      promise.setValue(result.childInodePtr);
    }
  }
}

ImmediateFuture<VirtualInode> TreeInode::getOrFindChild(
    PathComponentPiece name,
    const ObjectFetchContextPtr& context,
    bool loadInodes) {
  TraceBlock block("getOrFindChild");

#ifndef _WIN32
  if (name == kDotEdenName && getNodeId() != kRootNodeId) {
    // If they ask for `.eden` in any subdir, return the magical
    // this-dir symlink inode that resolves to the path to the
    // root/.eden path.  We do this outside of the block below
    // because getInodeSlow() will call TreeInode::getOrFindChild()
    // recursively, and it is cleaner to break this logic out
    // separately.
    return getMount()
        ->getInodeSlow(".eden/this-dir"_relpath, context)
        .thenValue([](auto&& inode) { return VirtualInode{std::move(inode)}; });
  }
#endif // !_WIN32
  return tryRlockCheckBeforeUpdate<ImmediateFuture<VirtualInode>>(
             contents_,
             [&](const auto& contents)
                 -> std::optional<ImmediateFuture<VirtualInode>> {
               return rlockGetOrFindChild(contents, name, context, loadInodes);
             },
             [&](auto& contents) -> ImmediateFuture<VirtualInode> {
               auto result = loadChild(contents, name, context);
               // it's important the code between loadChild and loadChildCleanUp
               // is no throw. We need to perform the loadChildCleanUp now
               // regardless of exception.
               contents.unlock();
               loadChildCleanUp(name, std::move(result.second));
               return ImmediateFuture<InodePtr>{std::move(result.first)}
                   .thenValue([](auto&& inode) { return VirtualInode{inode}; });
             })
      .ensure([b = std::move(block)]() mutable { b.close(); });
}

std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>
TreeInode::getChildren(const ObjectFetchContextPtr& context, bool loadInodes) {
  // We could optimize this to take the rlock first and try to get all the
  // VirtualInode with out loading inodes. This would allow for higher
  // concurrency. However, this will significantly increase code
  // complexity and can make non concurrent requests more expensive. We should
  //  perf in production before making this change: T125563920

  std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>> result;
  std::vector<std::pair<PathComponent, TreeInode::LoadChildCleanUp>>
      inodeLoadCleanUps;

  {
    // we always want to clean up the loads for as many of these inodes as we
    // can once the contents lock is dropped. This ensures even on exception
    // inode loads are completed. Note: the wlock must be taken after this scope
    // exit declaration, so the scope exit will be performed after the lock is
    // released.
    SCOPE_EXIT {
      for (auto& cleanUp : inodeLoadCleanUps) {
        loadChildCleanUp(cleanUp.first, std::move(cleanUp.second));
      }
    };
    auto contents = contents_.wlock();
    result.reserve(contents->entries.size());
    inodeLoadCleanUps.reserve(contents->entries.size());
    for (const auto& entry : contents->entries) {
      auto virtualInode =
          rlockGetOrFindChild(*contents, entry.first, context, loadInodes);
      if (virtualInode) {
        result.push_back(
            std::make_pair(entry.first, std::move(virtualInode.value())));
      } else {
        auto childResult = loadChild(contents, entry.first, context);
        // inodeLoadCleanUps.push_back must be no-except to guarantee
        // the cleanup will run if result.push_back below throws.
        XCHECK_LT(inodeLoadCleanUps.size(), inodeLoadCleanUps.capacity());
        inodeLoadCleanUps.push_back(
            std::make_pair(entry.first, std::move(childResult.second)));

        result.push_back(std::make_pair(
            entry.first,
            ImmediateFuture<InodePtr>{std::move(childResult.first)}.thenValue(
                [](auto&& inode) { return VirtualInode{inode}; })));
      }
    }
  }
  return result;
}

ImmediateFuture<InodePtr> TreeInode::getOrLoadChild(
    PathComponentPiece name,
    const ObjectFetchContextPtr& context) {
  return getOrFindChild(name, context, true).thenValue([](auto&& virtualInode) {
    return virtualInode.asInodePtr();
  });
}

ImmediateFuture<TreeInodePtr> TreeInode::getOrLoadChildTree(
    PathComponentPiece name,
    const ObjectFetchContextPtr& context) {
  return getOrLoadChild(name, context).thenValue([](InodePtr child) {
    auto treeInode = child.asTreePtrOrNull();
    if (!treeInode) {
      return ImmediateFuture<TreeInodePtr>{
          folly::Try<TreeInodePtr>{InodeError(ENOTDIR, child)}};
    }
    return ImmediateFuture<TreeInodePtr>{std::move(treeInode)};
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
  explicit LookupProcessor(
      RelativePathPiece path,
      ObjectFetchContextPtr context)
      : path_{path},
        iterRange_{path_.components()},
        iter_{iterRange_.begin()},
        context_{std::move(context)} {}

  ImmediateFuture<InodePtr> next(TreeInodePtr tree) {
    auto name = *iter_++;
    if (iter_ == iterRange_.end()) {
      return tree->getOrLoadChild(name, context_);
    } else {
      return tree->getOrLoadChildTree(name, context_)
          .thenValue(
              [this](TreeInodePtr tree) { return next(std::move(tree)); });
    }
  }

 private:
  RelativePath path_;
  RelativePath::base_type::component_iterator_range iterRange_;
  RelativePath::base_type::component_iterator iter_;
  ObjectFetchContextPtr context_;
};
} // namespace

ImmediateFuture<InodePtr> TreeInode::getChildRecursive(
    RelativePathPiece path,
    const ObjectFetchContextPtr& context) {
  auto pathStr = path.view();
  if (pathStr.empty()) {
    return inodePtrFromThis();
  }

  auto processor = std::make_unique<LookupProcessor>(path, context.copy());
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
  XDCHECK(
      !ent.getInode() || ent.getInode()->getNodeId() == ent.getInodeNumber())
      << "inode number mismatch: " << ent.getInode()->getNodeId()
      << " != " << ent.getInodeNumber();
  return ent.getInodeNumber();
}

void TreeInode::loadUnlinkedChildInode(
    PathComponentPiece name,
    InodeNumber number,
    std::optional<ObjectId> hash,
    mode_t mode) {
  try {
    InodeMap::PromiseVector promises;
    InodePtr inodePtr;

    if (!S_ISDIR(mode)) {
      auto file = std::make_unique<FileInode>(
          number,
          inodePtrFromThis(),
          name,
          mode,
          std::nullopt,
          hash ? &*hash : nullptr);
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
        if (!overlayContents.empty()) {
          // Should be impossible, but worth checking for
          // defensive purposes!
          throw std::runtime_error(
              "unlinked dir inode should have no children");
        }
      }

      auto tree = std::make_unique<TreeInode>(
          number,
          inodePtrFromThis(),
          name,
          mode,
          std::nullopt,
          std::move(overlayContents),
          hash ? std::optional<ObjectId>{*hash} : std::nullopt);
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
  std::optional<PathComponent> inodeName;
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

    inodeName = copyCanonicalInodeName(iter);
    name = inodeName->piece();
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

    // loadChildInode is called by InodeMap during FUSE_LOOKUP processing. Pass
    // a null fetch context because we don't need to record statistics.
    static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
        "TreeInode::loadChildInode");
    future = startLoadingInodeNoThrow(entry, name, context);
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
    PathComponentPiece name,
    const ObjectFetchContextPtr& fetchContext) noexcept {
  // The callers of startLoadingInodeNoThrow() need to make sure that they
  // always call InodeMap::inodeLoadComplete() or InodeMap::inodeLoadFailed()
  // afterwards.
  //
  // It simplifies their logic to guarantee that we never throw an exception,
  // and always return a Future object.  Therefore we simply wrap
  // startLoadingInode() and convert any thrown exceptions into Future.
  try {
    return startLoadingInode(entry, name, fetchContext);
  } catch (...) {
    // It's possible that makeFuture() itself could throw, but this only
    // happens on out of memory, in which case the whole process is pretty much
    // hosed anyway.
    return makeFuture<unique_ptr<InodeBase>>(
        folly::exception_wrapper{std::current_exception()});
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
    auto it = tree.find(entry.first);
    if (it == tree.cend()) {
      differences.insert(fmt::format("- {}", entry.first));
    }
  }
  for (const auto& entry : tree) {
    if (!dir.count(entry.first)) {
      differences.insert(fmt::format("+ {}", entry.first));
    }
  }
  return std::vector<std::string>{differences.begin(), differences.end()};
}

std::optional<std::vector<std::string>> findEntryDifferences(
    const DirContents& dir,
    const Tree& tree) {
  // Avoid allocations in the case where the tree and dir agree.
  if (dir.size() != tree.size()) {
    return computeEntryDifferences(dir, tree);
  }
  for (const auto& entry : dir) {
    auto it = tree.find(entry.first);
    if (it == tree.cend()) {
      return computeEntryDifferences(dir, tree);
    }
  }
  return std::nullopt;
}

Future<unique_ptr<InodeBase>> TreeInode::startLoadingInode(
    const DirEntry& entry,
    PathComponentPiece name,
    const ObjectFetchContextPtr& fetchContext) {
  XLOG(DBG5) << "starting to load inode " << entry.getInodeNumber() << ": "
             << getLogPath() << " / \"" << name << "\"";
  XDCHECK(entry.getInode() == nullptr);
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
        entry.getHashPtr());
  }

  if (!entry.isMaterialized()) {
    return getObjectStore()
        .getTree(entry.getHash(), fetchContext)
        .semi()
        .via(&folly::QueuedImmediateExecutor::instance())
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

              // If the directory we loaded from overlay is empty, there is no
              // need to compare them and we can just use the version from
              // backing store. The differences between nonexistent overlay and
              // empty directory does not matter here.
              if (!overlayDir.empty()) {
                // Compare the Tree and the Dir from the overlay.  If they
                // differ, something is wrong, so log the difference.
                if (auto differences =
                        findEntryDifferences(overlayDir, *tree)) {
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
                    std::move(overlayDir),
                    treeHash);
              }

              return make_unique<TreeInode>(
                  number, self, childName, entryMode, std::move(tree));
            });
  }

  // The entry is materialized, so data must exist in the overlay.
  auto overlayDir = loadOverlayDir(entry.getInodeNumber());
  return make_unique<TreeInode>(
      entry.getInodeNumber(),
      inodePtrFromThis(),
      name,
      entry.getInitialMode(),
      std::nullopt,
      std::move(overlayDir),
      std::nullopt);
}

void TreeInode::materialize(const RenameLock* renameLock) {
  // Start timing how long the materialize event takes before adding to TraceBus
  auto startTime = std::chrono::system_clock::now();

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
      getMount()->publishInodeTraceEvent(InodeTraceEvent(
          startTime,
          getNodeId(),
          InodeType::TREE,
          InodeEventType::MATERIALIZE,
          InodeEventProgress::START,
          getLocationInfo(*renameLock).name));
      contents->setMaterialized();
      saveOverlayDir(contents->entries);
    }

    // Mark ourself materialized in our parent directory (if we have one)
    auto loc = getLocationInfo(*renameLock);
    if (loc.parent && !loc.unlinked) {
      loc.parent->childMaterialized(*renameLock, loc.name);
    }

    // Finished materializing so publish event to TraceBus
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        getNodeId(),
        InodeType::TREE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::END,
        loc.name));
  }
}

/* If we don't yet have an overlay entry for this portion of the tree,
 * populate it from the Tree.  In order to materialize a dir we have
 * to also materialize its parents. */
void TreeInode::childMaterialized(
    const RenameLock& renameLock,
    PathComponentPiece childName) {
  auto startTime = std::chrono::system_clock::now();
  bool wasAlreadyMaterialized;
  {
    auto contents = contents_.wlock();
    wasAlreadyMaterialized = contents->isMaterialized();
    if (!wasAlreadyMaterialized) {
      getMount()->publishInodeTraceEvent(InodeTraceEvent(
          startTime,
          getNodeId(),
          InodeType::TREE,
          InodeEventType::MATERIALIZE,
          InodeEventProgress::START,
          getLocationInfo(renameLock).name));
    }

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

  // Materialize parent and publish materialization event only if newly
  // materialized
  if (!wasAlreadyMaterialized) {
    // If we have a parent directory, ask our parent to materialize itself
    // and mark us materialized when it does so.
    auto location = getLocationInfo(renameLock);
    if (location.parent && !location.unlinked) {
      location.parent->childMaterialized(renameLock, location.name);
    }

    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        getNodeId(),
        InodeType::TREE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::END,
        location.name));
  }
}

void TreeInode::childDematerialized(
    const RenameLock& renameLock,
    PathComponentPiece childName,
    ObjectId childScmHash) {
  auto startTime = std::chrono::system_clock::now();
  bool wasAlreadyMaterialized;
  {
    auto contents = contents_.wlock();
    wasAlreadyMaterialized = contents->isMaterialized();
    if (!wasAlreadyMaterialized) {
      getMount()->publishInodeTraceEvent(InodeTraceEvent(
          startTime,
          getNodeId(),
          InodeType::TREE,
          InodeEventType::MATERIALIZE,
          InodeEventProgress::START,
          getLocationInfo(renameLock).name));
    }

    auto iter = contents->entries.find(childName);
    if (iter == contents->entries.end()) {
      // This should never happen.
      // We should only get called with legitimate children names.
      EDEN_BUG() << "error attempting to dematerialize " << childName << " in "
                 << getLogPath() << ": entry not present";
    }

    auto& childEntry = iter->second;
    // Should this call ObjectStore::areObjectsKnownIdentical? No, even if IDs
    // are compatible, we want to migrate our inode to the new ID scheme, which
    // requires writing it to the overlay.
    if (!childEntry.isMaterialized() &&
        childEntry.getHash().bytesEqual(childScmHash)) {
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

  // Materialize parent and publish materialization event only if newly
  // materialized
  if (!wasAlreadyMaterialized) {
    // We are newly materialized now.
    // If we have a parent directory, ask our parent to materialize itself
    // and mark us materialized when it does so.
    auto location = getLocationInfo(renameLock);
    if (location.parent && !location.unlinked) {
      location.parent->childMaterialized(renameLock, location.name);
    }
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        getNodeId(),
        InodeType::TREE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::END,
        location.name));
  }
}

Overlay* TreeInode::getOverlay() const {
  return getMount()->getOverlay();
}

DirContents TreeInode::loadOverlayDir(InodeNumber inodeNumber) const {
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
  auto dir = buildDirFromTree(
      tree, overlay, mount->getCheckoutConfig()->getCaseSensitive());
  // buildDirFromTree just allocated inode numbers; they should be saved.
  overlay->saveOverlayDir(inodeNumber, dir);
  return dir;
}

DirContents TreeInode::buildDirFromTree(
    const Tree* tree,
    Overlay* overlay,
    CaseSensitivity caseSensitive) {
  XCHECK(tree);

  // A future optimization is for this code to allocate all of the inode numbers
  // at once and then dole them out, one per entry. It would reduce the number
  // of atomic operations from N to 1, though if the atomic is issued with the
  // other work this loop is doing it may not matter much.

  DirContents dir(caseSensitive);
  // TODO: O(N^2)
  for (const auto& treeEntry : *tree) {
    dir.emplace(
        treeEntry.first,
        modeFromTreeEntryType(treeEntry.second.getType()),
        overlay->allocateInodeNumber(),
        treeEntry.second.getHash());
  }
  return dir;
}

FileInodePtr TreeInode::createImpl(
    folly::Synchronized<TreeInodeState>::LockedPtr contents,
    PathComponentPiece name,
    mode_t mode,
    FOLLY_MAYBE_UNUSED ByteRange fileContents,
    InvalidationRequired invalidate,
    std::chrono::system_clock::time_point startTime) {
#ifndef _WIN32
  // This relies on the fact that the dotEdenInodeNumber field of EdenMount is
  // not defined until after EdenMount finishes configuring the .eden directory.
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis(), name);
  }
#endif
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
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        childNumber,
        InodeType::FILE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::START,
        targetName));

#ifndef _WIN32
    // Create the overlay file before we insert the file into our entries map.
    auto file = getOverlay()->createOverlayFile(childNumber, fileContents);
#endif

    auto now = getNow();
    auto inodeTimestamps = InodeTimestamps{now};

    // Record the new entry
    auto insertion = contents->entries.emplace(name, mode, childNumber);
    XCHECK(insertion.second)
        << "we already confirmed that this entry did not exist above";
    auto& entry = insertion.first->second;

    inode = FileInodePtr::makeNew(
        childNumber, this->inodePtrFromThis(), name, mode, inodeTimestamps);

    entry.setInode(inode.get());
    getInodeMap()->inodeCreated(inode);

    updateMtimeAndCtimeLocked(contents->entries, now);
#ifndef _WIN32
    getMount()->getServerState()->getFaultInjector().check(
        "createInodeSaveOverlay", name);
#endif

    getOverlay()->addChild(getNodeId(), *insertion.first, contents->entries);

    // Once the overlay is fully updated, the inode is materialized so we can
    // publish this to TraceBus
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        childNumber,
        InodeType::FILE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::END,
        targetName));
  }

  if (InvalidationRequired::Yes == invalidate) {
    invalidateChannelEntryCache(*contents, name, std::nullopt)
        .throwUnlessValue();
    // Make sure that the directory cache is invalidated so a subsequent
    // readdir will see the added file.
    invalidateChannelDirCache(*contents).get();
  }

  getMount()->getJournal().recordCreated(targetName);

  return inode;
}

#ifndef _WIN32
// Eden doesn't support symlinks on Windows

FileInodePtr TreeInode::symlink(
    PathComponentPiece name,
    folly::StringPiece symlinkTarget,
    InvalidationRequired invalidate) {
  // symlink creates a newly materialized file in createImpl. We count this as
  // an inode materialization event to publish to TraceBus, which we begin
  // timing here before the parent tree inode materializes
  auto startTime = std::chrono::system_clock::now();

  validatePathComponentLength(name);
  materialize();

  {
    // Acquire our contents lock
    auto contents = contents_.wlock();
    const mode_t mode = S_IFLNK | 0770;
    return createImpl(
        std::move(contents),
        name,
        mode,
        ByteRange{symlinkTarget},
        invalidate,
        startTime);
  }
}
#endif

FileInodePtr TreeInode::mknod(
    PathComponentPiece name,
    mode_t mode,
    dev_t dev,
    InvalidationRequired invalidate) {
  // mknod creates a newly materialized file in createImpl. We count this as an
  // inode materialization event to publish to TraceBus, which we begin timing
  // here before the parent tree inode materializes
  auto startTime = std::chrono::system_clock::now();

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
    return createImpl(
        std::move(contents), name, mode, ByteRange{}, invalidate, startTime);
  }
}

TreeInodePtr TreeInode::mkdir(
    PathComponentPiece name,
    mode_t mode,
    InvalidationRequired invalidate) {
  // A new materialized subtree is created in mkdir. We count this as a new
  // materializion event to publish to TraceBus which we begin timing.
  auto startTime = std::chrono::system_clock::now();

#ifndef _WIN32
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis(), name);
  }
#endif
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

    if (InvalidationRequired::Yes == invalidate) {
      invalidateChannelEntryCache(*contents, name, std::nullopt)
          .throwUnlessValue();
      invalidateChannelDirCache(*contents).get();
    }

    // Allocate an inode number
    auto childNumber = getOverlay()->allocateInodeNumber();
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        childNumber,
        InodeType::TREE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::START,
        targetName));

    // The mode passed in by the caller may not have the file type bits set.
    // Ensure that we mark this as a directory.
    mode = S_IFDIR | (07777 & mode);

    // Store the overlay entry for this dir
    DirContents emptyDir(getMount()->getCheckoutConfig()->getCaseSensitive());
    saveOverlayDir(childNumber, emptyDir);

    // Add a new entry to contents_.entries
    auto emplaceResult = contents->entries.emplace(name, mode, childNumber);
    XCHECK(emplaceResult.second)
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
    getOverlay()->addChild(
        getNodeId(), *emplaceResult.first, contents->entries);

    // Once the overlay is fully updated, the inode is materialized so we can
    // publish this to TraceBus
    getMount()->publishInodeTraceEvent(InodeTraceEvent(
        startTime,
        childNumber,
        InodeType::TREE,
        InodeEventType::MATERIALIZE,
        InodeEventProgress::END,
        targetName));
  }

  getMount()->getJournal().recordCreated(targetName);

  return newChild;
}

ImmediateFuture<folly::Unit> TreeInode::unlink(
    PathComponentPiece name,
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context) {
  return getOrLoadChild(name, context)
      .thenValue([self = inodePtrFromThis(),
                  childName = PathComponent{name},
                  invalidate,
                  context = context.copy()](const InodePtr& child) mutable {
        return self->removeImpl<FileInodePtr>(
            std::move(childName), child, invalidate, 1, context);
      });
}

ImmediateFuture<folly::Unit> TreeInode::rmdir(
    PathComponentPiece name,
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context) {
  return getOrLoadChild(name, context)
      .thenValue([self = inodePtrFromThis(),
                  childName = PathComponent{name},
                  invalidate,
                  context = context.copy()](const InodePtr& child) mutable {
        return self->removeImpl<TreeInodePtr>(
            std::move(childName), child, invalidate, 1, context);
      });
}

void TreeInode::removeAllChildrenRecursively(
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context,
    const RenameLock& renameLock) {
  // TODO: Unconditional materialization is slightly conservative. If the
  // BackingStore Tree is empty, then this function can return without
  // materializing.
  materialize(&renameLock);
#ifndef _WIN32
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis());
  }
#endif

  std::vector<TreeInodePtr> loadedTreeNodes;
  // Step 1, collect children nodes who are tree and loaded
  {
    auto contents = contents_.rlock();
    for (auto& entry : contents->entries) {
      if (auto asTreePtr = entry.second.asTreePtrOrNull()) {
        loadedTreeNodes.push_back(std::move(asTreePtr));
      }
    }
  }

  // Step 2, Clear contents in the child folders
  for (auto& treeNode : loadedTreeNodes) {
    treeNode->removeAllChildrenRecursively(invalidate, context, renameLock);
  }

  loadedTreeNodes.clear();

  // Step 3, Now all child nodes are removable, unless one of the directories
  // had a new entry added while the contents lock was not held.
  auto contents = contents_.wlock();
  auto it = contents->entries.begin();
  while (it != contents->entries.end()) {
    auto inodeNum = it->second.getInodeNumber();
    bool isDir = it->second.isDirectory();
    if (it->second.getInode()) {
      // If a treeInode is not empty, i.e. files were added to the tree
      // between step2 and step3, an exception will be thrown.

      // TODO: There's a race here: checkPreRemove acquires the child's
      // contents lock but then releases it after the check. Thus, there's a
      // window where the child can gain an entry being unlinked, which breaks
      // EdenFS's internal data model. This code should acquire the child's
      // contents lock and hold it across the unlink operation.
      //
      // TODO: Have checkPreRemove take a DirContents& to ensure the contents
      // lock is acquired by the parent, and encourage holding it across the
      // unlink operation.
      //
      // Be careful, here we obtains TreeInode* instead of TreeIndePtr to avoid
      // deference of TreeInodePtr, otherwise there could be a deadlock on
      // getting the parent location info when deference.
      if (TreeInode* asTreePtr = it->second.asTreeOrNull()) {
        int checkResult = checkPreRemove(*asTreePtr);
        if (checkResult != 0) {
          throw InodeError(checkResult, InodePtr::newPtrLocked(asTreePtr));
        }
      }

      auto inode = it->second.getInode();
      inode->markUnlinked(this, it->first, renameLock);
    }
    // Erase from contents must happen right after markUnlink
    it = contents->entries.erase(it);

    if (isDir) {
      getOverlay()->recursivelyRemoveOverlayDir(inodeNum);
    } else {
      getOverlay()->removeOverlayFile(inodeNum);
    }
  }

  if (InvalidationRequired::Yes == invalidate) {
    invalidateChannelDirCache(*contents).get();
  }
  updateMtimeAndCtimeLocked(contents->entries, getNow());
  getOverlay()->removeChildren(getNodeId(), contents->entries);
}

InodePtr TreeInode::tryRemoveUnloadedChild(
    PathComponentPiece name,
    InvalidationRequired invalidate) {
#ifndef _WIN32
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    throw InodeError(EPERM, inodePtrFromThis());
  }
#endif
  auto contents = contents_.wlock();

  auto it = contents->entries.find(name);
  if (it == contents->entries.end()) {
    throw InodeError(ENOENT, inodePtrFromThis(), name);
  }

  auto inodeName = copyCanonicalInodeName(it);
  auto inodeNumber = it->second.getInodeNumber();

  if (auto node = it->second.getInodePtr()) {
    // The child has a loaded! Fall back to the slow path.
    return node;
  }

  contents->entries.erase(it);
  if (InvalidationRequired::Yes == invalidate) {
    invalidateChannelEntryCache(*contents, inodeName, inodeNumber)
        .throwUnlessValue();
    invalidateChannelDirCache(*contents).get();
  }

  updateMtimeAndCtimeLocked(contents->entries, getNow());
  if (it->second.isDirectory()) {
    getOverlay()->recursivelyRemoveOverlayDir(inodeNumber);
  } else {
    getOverlay()->removeOverlayFile(inodeNumber);
  }
  getOverlay()->removeChild(getNodeId(), name, contents->entries);
  return nullptr;
}

ImmediateFuture<folly::Unit> TreeInode::removeRecursivelyNoFlushInvalidation(
    PathComponentPiece name,
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context) {
  // Fast return if the node is unloaded and removed
  auto child = tryRemoveUnloadedChild(name, invalidate);
  if (!child) {
    return folly::unit;
  }

  if (child.asFilePtrOrNull()) {
    return inodePtrFromThis()->removeImpl<FileInodePtr>(
        PathComponent{name}, std::move(child), invalidate, 1, context);
  } else {
    {
      auto renameLock = inodePtrFromThis()->getMount()->acquireRenameLock();
      child.asTreePtr()->removeAllChildrenRecursively(
          invalidate, context, renameLock);
    }
    return inodePtrFromThis()->removeImpl<TreeInodePtr>(
        PathComponent{name}, std::move(child), invalidate, 1, context);
  }
}

ImmediateFuture<folly::Unit> TreeInode::removeRecursively(
    PathComponentPiece name,
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context) {
  return this->removeRecursivelyNoFlushInvalidation(name, invalidate, context)
      .thenValue(
          [self = inodePtrFromThis(),
           invalidate](folly::Unit&&) -> ImmediateFuture<folly::Unit> {
            if (invalidate == InvalidationRequired::Yes) {
              return self->getMount()->flushInvalidations();
            }
            return folly::unit;
          });
}

template <typename InodePtrType>
ImmediateFuture<folly::Unit> TreeInode::removeImpl(
    PathComponent name,
    InodePtr childBasePtr,
    InvalidationRequired invalidate,
    unsigned int attemptNum,
    const ObjectFetchContextPtr& context) {
  // Make sure the child is of the desired type
  auto child = childBasePtr.asSubclassPtrOrNull<InodePtrType>();
  if (!child) {
    return ImmediateFuture<Unit>{folly::Try<Unit>{
        InodeError{InodePtrType::InodeType::WRONG_TYPE_ERRNO, childBasePtr}}};
  }

  // Verify that we can remove the child before we materialize ourself
  int checkResult = checkPreRemove(*child);
  if (checkResult != 0) {
    return ImmediateFuture<Unit>{
        folly::Try<Unit>{InodeError{checkResult, child}}};
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
    return ImmediateFuture<Unit>{
        folly::Try<Unit>{InodeError{ENOENT, inodePtrFromThis()}}};
  }
  auto targetName = myPath.value() + name;

  // The entry in question may have been renamed since we loaded the child
  // Inode pointer.  If this happens, that's fine, and we just want to go ahead
  // and try removing whatever is present with this name anyway.
  //
  // Therefore leave the child parameter for tryRemoveChild() as null, and let
  // it remove whatever it happens to find with this name.
  const InodePtrType nullChildPtr;
  int errnoValue = tryRemoveChild(renameLock, name, nullChildPtr, invalidate);
  if (errnoValue == 0) {
    // We successfully removed the child.
    // Record the change in the journal.
    getMount()->getJournal().recordRemoved(targetName);

    return folly::unit;
  }

  // EBADF means that the child in question has been replaced since we looked
  // it up earlier, and the child inode now at this location is not loaded.
  if (errnoValue != EBADF) {
    return ImmediateFuture<Unit>{
        folly::Try<Unit>{InodeError{errnoValue, inodePtrFromThis(), name}}};
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
  auto childFuture = getOrLoadChild(name, context);
  return std::move(childFuture)
      .thenValue([self = inodePtrFromThis(),
                  childName = PathComponent{std::move(name)},
                  invalidate,
                  attemptNum,
                  context = context.copy()](const InodePtr& loadedChild) {
        return self->removeImpl<InodePtrType>(
            childName, loadedChild, invalidate, attemptNum + 1, context);
      });
}

template <typename InodePtrType>
int TreeInode::tryRemoveChild(
    const RenameLock& renameLock,
    PathComponentPiece name,
    InodePtrType child,
    InvalidationRequired invalidate) {
  materialize(&renameLock);

#ifndef _WIN32
  // prevent unlinking files in the .eden directory
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return EPERM;
  }
#endif // !_WIN32

  // Lock our contents in write mode.
  // We will hold it for the duration of the unlink.
  std::unique_ptr<InodeBase> deletedInode;
  std::optional<PathComponent> inodeName;
  {
    auto contents = contents_.wlock();

    // Make sure that this name still corresponds to the child inode we just
    // looked up.
    auto entIter = contents->entries.find(name);
    if (entIter == contents->entries.end()) {
      return ENOENT;
    }
    inodeName = copyCanonicalInodeName(entIter);
    name = inodeName->piece();
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
    auto checkError = checkPreRemove(*child);
    if (checkError != 0) {
      return checkError;
    }

    // Flush the kernel cache for this entry if requested.
    // Since invalidation can fail on ProjectedFS, do it while holding the
    // TreeInode write lock and before updating the contents.
    if (InvalidationRequired::Yes == invalidate) {
      auto success =
          invalidateChannelEntryCache(*contents, name, ent.getInodeNumber());
      if (success.hasException()) {
        return EIO;
      }

      success = invalidateChannelDirCache(*contents).getTry();
      if (success.hasException()) {
        return EIO;
      }
    }

    // Inform the child it is now unlinked
    deletedInode = child->markUnlinked(this, name, renameLock);

    // Remove it from our entries list
    contents->entries.erase(entIter);

    // We want to update mtime and ctime of parent directory after removing the
    // child.
    updateMtimeAndCtimeLocked(contents->entries, getNow());
    getOverlay()->removeChild(getNodeId(), name, contents->entries);
  }
  deletedInode.reset();

  // We have successfully removed the entry.
  return 0;
}

int TreeInode::checkPreRemove(const TreeInode& child) {
  // Lock the child contents, and make sure they are empty
  auto childContents = child.contents_.rlock();
  if (!childContents->entries.empty()) {
    return ENOTEMPTY;
  }
  return 0;
}

int TreeInode::checkPreRemove(const FileInode& /* child */) {
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

  TreeInodeState& srcInodeState() {
    return *srcContentsLock_;
  }

  TreeInodeState& dstInodeState() {
    return *destContentsLock_;
  }

  const PathMap<DirEntry>::iterator& destChildIter() const {
    return destChildIter_;
  }
  InodeBase* destChild() const {
    XDCHECK(destChildExists());
    return destChildIter_->second.getInode();
  }

  bool destChildExists() const {
    return destChildIter_ != destContents_->end();
  }
  bool destChildIsDirectory() const {
    XDCHECK(destChildExists());
    return destChildIter_->second.isDirectory();
  }
  bool destChildIsEmpty() const {
    XDCHECK(destChildContents_);
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

ImmediateFuture<Unit> TreeInode::rename(
    PathComponentPiece name,
    TreeInodePtr destParent,
    PathComponentPiece destName,
    InvalidationRequired invalidate,
    const ObjectFetchContextPtr& context) {
#ifndef _WIN32
  if (getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return ImmediateFuture<Unit>{
        folly::Try<Unit>{InodeError{EPERM, inodePtrFromThis(), name}}};
  }
  if (destParent->getNodeId() == getMount()->getDotEdenInodeNumber()) {
    return ImmediateFuture<Unit>{
        folly::Try<Unit>{InodeError{EPERM, destParent, destName}}};
  }
#endif
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
      return ImmediateFuture<Unit>{
          folly::Try<Unit>{InodeError{ENOENT, inodePtrFromThis(), name}}};
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
          return ImmediateFuture<Unit>{
              folly::Try<Unit>{InodeError{ENOTDIR, destParent, destName}}};
        } else if (
            locks.destChild() != srcEntry.getInode() &&
            !locks.destChildIsEmpty()) {
          XLOG(DBG4) << "attempted to rename directory " << getLogPath() << "/"
                     << name << " over non-empty directory "
                     << destParent->getLogPath() << "/" << destName;
          return ImmediateFuture<Unit>{
              folly::Try<Unit>{InodeError{ENOTEMPTY, destParent, destName}}};
        }
      }
    } else {
      // The source is not a directory.
      // The destination must not exist, or must not be a directory.
      if (locks.destChildExists() && locks.destChildIsDirectory()) {
        XLOG(DBG4) << "attempted to rename file " << getLogPath() << "/" << name
                   << " over directory " << destParent->getLogPath() << "/"
                   << destName;
        return ImmediateFuture<Unit>{
            folly::Try<Unit>{InodeError{EISDIR, destParent, destName}}};
      }
    }

    // Make sure the destination directory is not unlinked.
    if (destParent->isUnlinked()) {
      XLOG(DBG4) << "attempted to rename file " << getLogPath() << "/" << name
                 << " into deleted directory " << destParent->getLogPath()
                 << " ( as " << destName << ")";
      return ImmediateFuture<Unit>{
          folly::Try<Unit>{InodeError{ENOENT, destParent}}};
    }

    // Check to see if we need to load the source or destination inodes
    needSrc = !srcEntry.getInode();
    needDest = locks.destChildExists() && !locks.destChild();

    // If we don't have to load anything now, we can immediately perform the
    // rename.
    if (!needSrc && !needDest) {
      return doRename(
          std::move(locks), name, srcIter, destParent, destName, invalidate);
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
                         destNameCopy = destName.copy(),
                         invalidate,
                         context = context.copy()](auto&&) mutable {
    return self->rename(
        nameCopy, destParent, destNameCopy, invalidate, context);
  };

  if (needSrc && needDest) {
    auto srcFuture = getOrLoadChild(name, context);
    auto destFuture = destParent->getOrLoadChild(destName, context);

    return std::move(srcFuture).thenValue(
        [destFuture = std::move(destFuture),
         onLoadFinished = std::move(onLoadFinished)](auto&&) mutable {
          return std::move(destFuture).thenValue(std::move(onLoadFinished));
        });
  } else if (needSrc) {
    return getOrLoadChild(name, context).thenValue(std::move(onLoadFinished));
  } else {
    XCHECK(needDest);
    return destParent->getOrLoadChild(destName, context)
        .thenValue(std::move(onLoadFinished));
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

ImmediateFuture<Unit> TreeInode::doRename(
    TreeRenameLocks&& locks,
    PathComponentPiece srcName,
    PathMap<DirEntry>::iterator srcIter,
    TreeInodePtr destParent,
    PathComponentPiece destName,
    InvalidationRequired invalidate) {
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
      return ImmediateFuture<Unit>{
          folly::Try<Unit>{InodeError{EINVAL, destParent, destName}}};
    }
  }

  // If the rename occurred outside of a FUSE request (unlikely), make sure to
  // invalidate the kernel caches.
  if (InvalidationRequired::Yes == invalidate) {
    invalidateChannelEntryCache(
        locks.srcInodeState(), srcName, srcIter->second.getInodeNumber())
        .throwUnlessValue();
    destParent
        ->invalidateChannelEntryCache(
            locks.dstInodeState(), destName, std::nullopt)
        .throwUnlessValue();

    invalidateChannelDirCache(locks.srcInodeState()).get();
    if (destParent.get() != this) {
      destParent->invalidateChannelDirCache(locks.dstInodeState()).get();
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
    XCHECK(ret.second);

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

  getOverlay()->renameChild(
      getNodeId(),
      destParent->getNodeId(),
      srcName,
      destName,
      *locks.srcContents(),
      *locks.destContents());

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
      XCHECK_NE(destChildContents_, nullptr);
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

#ifndef _WIN32
template <typename Fn>
bool TreeInode::readdirImpl(
    off_t off,
    const ObjectFetchContextPtr& context,
    Fn add) {
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
  prefetch(context);

  // Possible offset values are:
  //   0: start at the beginning
  //   1: start after .
  //   2: start after ..
  //   2+N: start after inode N

  if (off == 0) {
    if (!add(".", DirEntry{dtype_to_mode(dtype_t::Dir), getNodeId()}, 1)) {
      return false;
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
    if (!add("..", DirEntry{dtype_to_mode(dtype_t::Dir), parentNodeId}, 2)) {
      return false;
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

  // The provided FuseDirList has limited space. Add entries until no more fit.
  while (indices.size()) {
    std::pop_heap(indices.begin(), indices.end(), std::greater<>{});
    auto& [name, entry] = entries.begin()[indices.back().second];
    indices.pop_back();

    if (!add(name.view(), entry, entry.getInodeNumber().get() + 2)) {
      return false;
    }
  }

  return true;
}

FuseDirList TreeInode::fuseReaddir(
    FuseDirList&& list,
    off_t off,
    const ObjectFetchContextPtr& context) {
  readdirImpl(
      off,
      context,
      [&list](StringPiece name, const DirEntry& entry, uint64_t offset) {
        return list.add(
            name, entry.getInodeNumber().get(), entry.getDtype(), offset);
      });

  return std::move(list);
}

std::tuple<NfsDirList, bool> TreeInode::nfsReaddir(
    NfsDirList&& list,
    off_t off,
    const ObjectFetchContextPtr& context) {
  updateAtime();
  bool isEof = readdirImpl(
      off,
      context,
      [&list](StringPiece name, const DirEntry& entry, uint64_t offset) {
        return list.add(name, entry.getInodeNumber(), offset);
      });

  return {std::move(list), isEof};
}
#endif // _WIN32

InodeMap* TreeInode::getInodeMap() const {
  return getMount()->getInodeMap();
}

/*
On each level of the level order traversal, we search for a gitignore file, and
if it exists, we load it. This gitignore file is owned by a `std::<unique_ptr>`
on each level, and the file contents are stored within a `GitIgnoreStack`. Lower
levels of the tree are passed raw pointers to their parent's `GitIgnoreStack`,
and store this pointer after loading the file contents of that level. In other
words, a `GitIgnoreStack` contains a `GitIgnoreStack*` to the parent's
`GitIgnoreStack`, and the current level's gitignore file contents (if a
gitignore file exists). We are allowed to pass raw pointers derived from the
`std::<unique_ptr>` because the raw pointers are passed only to the childrens'
recursive calls and we are sure that the children calls will finish before we
return from the parent call.
*/
ImmediateFuture<Unit> TreeInode::diff(
    DiffContext* context,
    RelativePathPiece currentPath,
    std::vector<shared_ptr<const Tree>> trees,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  if (context->isCancelled()) {
    XLOG(DBG7) << "diff() on directory " << getLogPath()
               << " cancelled due to client request no longer being active";
    return folly::unit;
  }

  InodePtr inode;
  auto gitignoreInodeFuture = ImmediateFuture<InodePtr>::makeEmpty();
  vector<IncompleteInodeLoad> pendingLoads;
  {
    // We have to get a write lock since we may have to load
    // the .gitignore inode, which changes the entry status
    auto contents = contents_.wlock();

    // TODO: support trees.size() != 1
    XLOG(DBG7) << "diff() on directory " << getLogPath() << " (" << getNodeId()
               << ", "
               << (contents->isMaterialized()
                       ? "materialized"
                       : contents->treeHash->toLogString())
               << ") vs "
               << (trees.size() == 1 ? trees[0]->getHash().toLogString()
                                     : "null tree");

    // Check to see if we can short-circuit the diff operation if we have the
    // same hash as the tree we are being compared to.
    if (!contents->isMaterialized()) {
      for (auto& tree : trees) {
        if (getObjectStore().areObjectsKnownIdentical(
                contents->treeHash.value(), tree->getHash())) {
          // There are no changes in our tree or any children subtrees.
          return folly::unit;
        }
      }
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
          std::move(trees),
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
          std::move(trees),
          make_unique<GitIgnoreStack>(parentIgnore), // empty with no rules
          isIgnored);
    }

    XLOG(DBG7) << "Loading ignore file for " << getLogPath();
    inode = gitignoreEntry->getInodePtr();
    if (!inode) {
      gitignoreInodeFuture = loadChildLocked(
                                 contents->entries,
                                 kIgnoreFilename,
                                 *gitignoreEntry,
                                 pendingLoads,
                                 context->getFetchContext())
                                 .semi();
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
                    trees = std::move(trees),
                    parentIgnore,
                    isIgnored](InodePtr&& loadedInode) mutable {
          return self->loadGitIgnoreThenDiff(
              std::move(loadedInode),
              context,
              currentPath,
              std::move(trees),
              parentIgnore,
              isIgnored);
        });
  } else {
    return loadGitIgnoreThenDiff(
        std::move(inode),
        context,
        currentPath,
        std::move(trees),
        parentIgnore,
        isIgnored);
  }
}

ImmediateFuture<Unit> TreeInode::loadGitIgnoreThenDiff(
    InodePtr gitignoreInode,
    DiffContext* context,
    RelativePathPiece currentPath,
    std::vector<shared_ptr<const Tree>> trees,
    const GitIgnoreStack* parentIgnore,
    bool isIgnored) {
  return makeImmediateFutureWith([gitignoreInode = std::move(gitignoreInode),
                                  context] {
           auto fileInode = gitignoreInode.asFileOrNull();
           if (!fileInode) {
             XLOG(WARN)
                 << "loadGitIgnoreThenDiff() invoked with a non-file inode: "
                 << gitignoreInode->getLogPath();
             return makeImmediateFuture<std::string>(
                 InodeError(EISDIR, gitignoreInode));
           } else {
#ifndef _WIN32
             if (fileInode->getType() == dtype_t::Symlink) {
               return makeImmediateFuture<std::string>(
                   InodeError(EMLINK, gitignoreInode));
             }
#endif
             return fileInode->readAll(context->getFetchContext());
           }
         })
      .thenTry([self = inodePtrFromThis(),
                context,
                currentPath = RelativePath{currentPath}, // deep copy
                trees = std::move(trees),
                parentIgnore,
                isIgnored](
                   folly::Try<std::string> ignoreFileContentsTry) mutable {
        std::string ignoreFileContents;
        if (ignoreFileContentsTry.hasException()) {
          XLOG(WARN) << "error reading ignore file: "
                     << folly::exceptionStr(ignoreFileContentsTry.exception());
        } else {
          ignoreFileContents = std::move(ignoreFileContentsTry).value();
        }
        return self->computeDiff(
            self->contents_.wlock(),
            context,
            currentPath,
            std::move(trees),
            make_unique<GitIgnoreStack>(parentIgnore, ignoreFileContents),
            isIgnored);
      });
}

/*
This algorithm starts at the root `TreeInode` of the working directory and the
root source control commit `Tree`, traversing the trees in a level order
traversal. Per level of the tree, we loop over the children entries and
recursively process each entry as either added, removed, modified, ignored, or
hidden. We can also recognize that there has been no change and skip over that
child. We process children by constructing and collecting `DeferredDiffEntry`
objects using the children `TreeInode` objects and manually `run()`ning these.
If we are processing a file, we record this
added/removed/modified/ignored/hidden state in a callback, and extract this
collected information after the original `diff()` call has been completed.

In the case in which the working directory entry is not materialized, but it has
possibly been modified (for example, if a unmaterialized directory was moved, or
if we're calling diff with a source control commit that is far away from the
working directory parent), we can make an optimization. Since unmaterialized
inode entries still hold their commit hash, we can directly compare the working
directory entry's corresponding source control entry with the queried source
control commit's entry instead of materializing the inode entry to continue
through the working directory vs source control commit logic. We do this by
entering a different code path that acts similarly to the previously described
algorithm, but runs purely recursively instead of using `DeferredDiffEntries`
due to the fact that we do not need to worry about recursive lock holding since
the only time a lock is held in this path is when we load gitignore files.
*/
ImmediateFuture<Unit> TreeInode::computeDiff(
    folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
    DiffContext* context,
    RelativePathPiece currentPath,
    std::vector<shared_ptr<const Tree>> trees,
    std::unique_ptr<GitIgnoreStack> ignore,
    bool isIgnored) {
  XDCHECK(isIgnored || ignore != nullptr)
      << "the ignore stack is required if this directory is not ignored";

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
    //
    // This lock is held while we are processing the child
    // entries of the tree. The lock is released once we have constructed all of
    // the needed `DeferredDiffEntry` objects, and then these are manually ran,
    // which themselves call `diff()` with their internal `TreeInode`. This is
    // done so we are not recursively holding the `contents` lock, which could
    // lead to deadlock.
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

      if (!entryIgnored) {
        XLOG(DBG8) << "diff: untracked file: " << entryPath;
        context->callback->addedPath(entryPath, inodeEntry->getDtype());
      } else if (context->listIgnored) {
        XLOG(DBG9) << "diff: ignored file: " << entryPath;
        context->callback->ignoredPath(entryPath, inodeEntry->getDtype());
      } else {
        // Don't bother reporting this ignored file since
        // listIgnored is false.
      }

      if (inodeEntry->isDirectory()) {
        if (!entryIgnored || context->listIgnored) {
          if (auto childPtr = inodeEntry->getInodePtr()) {
            deferredEntries.emplace_back(
                DeferredDiffEntry::createUntrackedEntry(
                    context,
                    entryPath,
                    std::move(childPtr),
                    ignore.get(),
                    entryIgnored));
          } else if (inodeEntry->isMaterialized()) {
            ImmediateFuture<InodePtr> inodeFuture =
                self->loadChildLocked(
                        contents->entries,
                        name,
                        *inodeEntry,
                        pendingLoads,
                        context->getFetchContext())
                    .semi();
            deferredEntries.emplace_back(
                DeferredDiffEntry::createUntrackedEntry(
                    context,
                    entryPath,
                    std::move(inodeFuture),
                    ignore.get(),
                    entryIgnored));
          } else {
            // This entry is present locally but not in the source control tree.
            // The current Inode is not materialized so do not load inodes and
            // instead use the source control differ.

            // Collect this future to complete with other
            // deferred entries.
            deferredEntries.emplace_back(DeferredDiffEntry::createAddedScmEntry(
                context,
                entryPath,
                inodeEntry->getHash(),
                ignore.get(),
                entryIgnored));
          }
        }
      }
    };

    auto processRemoved = [&](const Tree::value_type& scmEntry) {
      XLOG(DBG5) << "diff: removed file: " << currentPath + scmEntry.first;
      context->callback->removedPath(
          currentPath + scmEntry.first, scmEntry.second.getDtype());
      if (scmEntry.second.isTree()) {
        deferredEntries.emplace_back(DeferredDiffEntry::createRemovedScmEntry(
            context, currentPath + scmEntry.first, scmEntry.second.getHash()));
      }
    };

    auto processBothPresent = [&](PathComponentPiece componentPath,
                                  std::vector<TreeEntry> scmEntries,
                                  DirEntry* inodeEntry) {
      XCHECK_GT(scmEntries.size(), 0ull);

      // We only need to know the ignored status if this is a directory.
      // If this is a regular file on disk and in source control, then it
      // is always included since it is already tracked in source control.
      bool entryIgnored = isIgnored;
      auto entryPath = currentPath + componentPath;
      if (!isIgnored && (inodeEntry->isDirectory() || scmEntries[0].isTree())) {
        auto fileType = inodeEntry->isDirectory() ? GitIgnore::TYPE_DIR
                                                  : GitIgnore::TYPE_FILE;
        auto ignoreStatus = ignore->match(entryPath, fileType);
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
            std::move(scmEntries),
            std::move(childInodePtr),
            ignore.get(),
            entryIgnored));
      } else if (inodeEntry->isMaterialized()) {
        // This inode is not loaded but is materialized.
        // We'll have to load it to confirm if it is the same or different.
        ImmediateFuture<InodePtr> inodeFuture =
            self->loadChildLocked(
                    contents->entries,
                    componentPath,
                    *inodeEntry,
                    pendingLoads,
                    context->getFetchContext())
                .semi();
        deferredEntries.emplace_back(DeferredDiffEntry::createModifiedEntry(
            context,
            entryPath,
            std::move(scmEntries),
            std::move(inodeFuture),
            ignore.get(),
            entryIgnored));
      } else {
        // If the inode is neither loaded nor materialized, then the inode
        // points at source control objects. At this point we check to see if
        // it's an exact match with any source control object. Otherwise we
        // just mark it as a diff against the first object we have.
        bool exactMatch = false;
        for (const auto& scmEntry : scmEntries) {
          if (
              // Eventually the mode will come from inode metadata storage,
              // not from the directory entry.  However, any
              // source-control-visible metadata changes will cause the inode to
              // be materialized, and the previous path will be taken.
              treeEntryTypeFromMode(inodeEntry->getInitialMode()) ==
                  scmEntry.getType() &&
              getObjectStore().areObjectsKnownIdentical(
                  inodeEntry->getHash(), scmEntry.getHash())) {
            exactMatch = true;
            break;
          }
        }

        const auto& scmEntry = scmEntries[0];

        if (exactMatch) {
          // This file or directory is unchanged.  We can skip it.
          XLOG(DBG9) << "diff: unchanged unloaded file: " << entryPath;
        } else if (inodeEntry->isDirectory()) {
          // This is a modified directory. Since it is not materialized we can
          // directly compare the source control objects.

          context->callback->modifiedPath(entryPath, inodeEntry->getDtype());
          // Collect this future to complete with other deferred entries.
          deferredEntries.emplace_back(
              DeferredDiffEntry::createModifiedScmEntry(
                  context,
                  entryPath,
                  scmEntry.getHash(),
                  inodeEntry->getHash(),
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
              context->callback->ignoredPath(entryPath, inodeEntry->getDtype());
            }
          } else {
            XLOG(DBG6) << "diff: directory --> untracked file: " << entryPath;
            context->callback->addedPath(entryPath, inodeEntry->getDtype());
          }
          context->callback->removedPath(entryPath, scmEntry.getDtype());
          deferredEntries.emplace_back(DeferredDiffEntry::createRemovedScmEntry(
              context, entryPath, scmEntry.getHash()));
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
            XLOG(DBG5) << "diff: file modified due to mode change: "
                       << entryPath;
            context->callback->modifiedPath(entryPath, inodeEntry->getDtype());
          } else {
            // TODO: Hopefully at some point we will track file sizes in the
            // parent TreeInode::Entry and the TreeEntry.  Once we have file
            // sizes, we could check for differing file sizes first, and
            // avoid loading the blob if they are different.
            deferredEntries.emplace_back(DeferredDiffEntry::createModifiedEntry(
                context,
                entryPath,
                scmEntry,
                inodeEntry->getHash(),
                inodeEntry->getDtype()));
          }
        }
      }
    };

    // Walk through the source control tree entries and our inode entries to
    // look for differences.
    //
    // This code relies on the fact that the source control entries and our
    // inode entries are both sorted in the same order.
    std::vector<Tree::const_iterator> scEnds;
    std::vector<Tree::const_iterator> scIters;
    scEnds.reserve(trees.size());
    scIters.reserve(trees.size());

    for (auto& tree : trees) {
      scEnds.push_back(tree->cend());
      scIters.push_back(tree->cbegin());
    }
    auto& inodeEntries = contents->entries;
    auto inodeIter = inodeEntries.begin();
    while (true) {
      const Tree::key_type* earliestPath =
          inodeIter != inodeEntries.end() ? &inodeIter->first : nullptr;
      DirContents::iterator* matchingInodeIter =
          inodeIter != inodeEntries.end() ? &inodeIter : nullptr;

      std::vector<Tree::const_iterator*> matchingScIters;

      // Find the earliest path in all the iterators, and record which iterators
      // have that path.
      for (size_t i = 0; i < scEnds.size(); i++) {
        auto& scEnd = scEnds[i];
        auto& scIter = scIters[i];
        if (scIter != scEnd) {
          if (!earliestPath) {
            earliestPath = &scIter->first;
            matchingScIters.push_back(&scIter);
          } else {
            auto compare = comparePathPiece(
                scIter->first, *earliestPath, context->getCaseSensitive());

            if (compare == CompareResult::BEFORE) {
              // If we find an earlier path, reset our current state.
              earliestPath = &scIter->first;
              matchingInodeIter = nullptr;
              matchingScIters.clear();
              matchingScIters.push_back(&scIter);
            } else if (compare == CompareResult::AFTER) {
              // If we find a later path, ignore it.
            } else {
              // If the path matches the earliest path we've seen, record it.
              matchingScIters.push_back(&scIter);
            }
          }
        }
      }

      // If there are no matches, then we've finished the entire walk.
      if (!matchingInodeIter && matchingScIters.size() == 0) {
        break;
      }

      if (!matchingInodeIter) { // If the inode doesn't have this path...
        if (matchingScIters.size() == scIters.size()) { // ...but all trees do..
          // ...then this entry is considered removed.
          processRemoved(**matchingScIters[0]);
        } else { // ...but not all trees do...
          // ...then this entry is considered unchanged, since some tree matches
          // the inode.
        }
      } else { // If the inode has this path...
        if (matchingScIters.size() == 0) { // ...but no trees do...
          // ...then the entry is considered untracked.
          processUntracked(inodeIter->first, &inodeIter->second);
        } else { // ...and some trees do as well...
          // ...then we need to compare this entry with the trees that have it.
          std::vector<TreeEntry> matchingTrees{};
          matchingTrees.reserve(matchingScIters.size());
          for (auto& scIter : matchingScIters) {
            matchingTrees.push_back((*scIter)->second);
          }
          processBothPresent(
              inodeIter->first, matchingTrees, &inodeIter->second);
        }
      }

      // Move everything forward that had the earliest path.
      if (matchingInodeIter) {
        ++(*matchingInodeIter);
      }
      for (auto& scIter : matchingScIters) {
        ++(*scIter);
      }
    }
  }

  // Finish setting up any load operations we started while holding the
  // contents_ lock above.
  for (auto& load : pendingLoads) {
    load.finish();
  }

  // Now process all of the deferred work.
  std::vector<ImmediateFuture<Unit>> deferredFutures;
  for (auto& entry : deferredEntries) {
    deferredFutures.push_back(entry->run());
  }

  // Wait on all of the deferred entries to complete.
  // Note that we explicitly move-capture the deferredFutures vector into this
  // callback, to ensure that the DeferredDiffEntry objects do not get
  // destroyed before they complete.
  return collectAll(std::move(deferredFutures))
      .thenValue([self = std::move(self),
                  currentPath = RelativePath{std::move(currentPath)},
                  context,
                  // Capture ignore to ensure it remains valid until all of our
                  // children's diff operations complete.
                  ignore = std::move(ignore),
                  deferredJobs = std::move(deferredEntries)](
                     std::vector<folly::Try<Unit>> results) {
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
        return folly::unit;
      });
}

Future<Unit> TreeInode::checkout(
    CheckoutContext* ctx,
    std::shared_ptr<const Tree> fromTree,
    std::shared_ptr<const Tree> toTree) {
  XLOG(DBG4) << "checkout: starting update of " << getLogPath() << ": "
             << (fromTree ? fromTree->getHash().toLogString() : "<none>")
             << " --> "
             << (toTree ? toTree->getHash().toLogString() : "<none>");

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
    actionFutures.emplace_back(action->run(ctx, &getObjectStore()));
  }

  ImmediateFuture<Unit> faultFuture =
      getMount()->getServerState()->getFaultInjector().checkAsync(
          "TreeInode::checkout", getLogPath(), ctx->isDryRun());
  folly::SemiFuture<vector<folly::Try<facebook::eden::InvalidationRequired>>>
      collectFuture = folly::collectAll(actionFutures);

  // Wait for all of the actions, and record any errors.
  return std::move(faultFuture)
      .semi()
      .toUnsafeFuture()
      .thenValue([collectFuture = std::move(collectFuture)](auto&&) mutable {
        return std::move(collectFuture);
      })
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

            auto invalidation = ImmediateFuture<folly::Unit>{folly::unit};
            if (wasDirectoryListModified) {
              // TODO(xavierd): In theory, this should be done before running
              // the futures, while holding the contents lock all the way. The
              // reason is that we in theory need to rollback what was done in
              // case we can't invalidate.
              {
                auto contents = self->contents_.wlock();
                self->updateMtimeAndCtimeLocked(
                    contents->entries, self->getNow());
                invalidation = self->invalidateChannelDirCache(*contents);
              }
              invalidation =
                  std::move(invalidation)
                      .thenTry([self, ctx](folly::Try<folly::Unit>&& success) {
                        if (success.hasException()) {
                          auto location =
                              self->getLocationInfo(ctx->renameLock());
                          ctx->addError(
                              location.parent.get(),
                              location.name,
                              success.exception());
                        }
                      });
            }

            auto fut = std::move(invalidation)
                           .thenValue([self,
                                       ctx,
                                       toTree = std::move(toTree),
                                       numErrors](auto&&) {
                             // Update our state in the overlay
                             self->saveOverlayPostCheckout(ctx, toTree.get());

                             XLOG(DBG4) << "checkout: finished update of "
                                        << self->getLogPath() << ": "
                                        << numErrors << " errors";
                           });

            if (fut.isReady()) {
              return folly::makeFuture(std::move(fut).getTry());
            } else {
              return std::move(fut).semi().via(
                  self->getMount()->getServerThreadPool().get());
            }
          });
}

bool TreeInode::canShortCircuitCheckout(
    CheckoutContext* ctx,
    const ObjectId& treeHash,
    const Tree* fromTree,
    const Tree* toTree) {
  if (ctx->isDryRun()) {
    // In a dry-run update we only care about checking for conflicts
    // with the fromTree state.  Since we aren't actually performing any
    // updates we can bail out early as long as there are no conflicts.
    if (fromTree) {
      return ctx->getObjectStore()->areObjectsKnownIdentical(
          treeHash, fromTree->getHash());
    } else {
      // There is no fromTree.  If we are already in the desired destination
      // state we don't have conflicts.  Otherwise we have to continue and
      // check for conflicts.
      return !toTree ||
          ctx->getObjectStore()->areObjectsKnownIdentical(
              treeHash, toTree->getHash());
    }
  }

  // For non-dry-run updates we definitely have to keep going if we aren't in
  // the desired destination state.
  if (!toTree ||
      !ctx->getObjectStore()->areObjectsKnownIdentical(
          treeHash, toTree->getHash())) {
    // If the objects are known different or not known identical, we must take
    // the slow path.
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
  return ctx->getObjectStore()->areObjectsKnownIdentical(
      treeHash, fromTree->getHash());
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
  Tree::container emptyEntries{
      getMount()->getCheckoutConfig()->getCaseSensitive()};
  auto oldIter = fromTree ? fromTree->cbegin() : emptyEntries.cbegin();
  auto oldEnd = fromTree ? fromTree->cend() : emptyEntries.cend();
  auto newIter = toTree ? toTree->cbegin() : emptyEntries.cbegin();
  auto newEnd = toTree ? toTree->cend() : emptyEntries.cend();
  while (true) {
    unique_ptr<CheckoutAction> action;

    if (oldIter == oldEnd) {
      if (newIter == newEnd) {
        // All Done
        break;
      }

      // This entry is present in the new tree but not the old one.
      action = processCheckoutEntry(
          ctx,
          *contents,
          nullptr,
          &*newIter,
          pendingLoads,
          wasDirectoryListModified);
      ++newIter;
    } else if (newIter == newEnd) {
      // This entry is present in the old tree but not the old one.
      action = processCheckoutEntry(
          ctx,
          *contents,
          &*oldIter,
          nullptr,
          pendingLoads,
          wasDirectoryListModified);
      ++oldIter;
    } else {
      auto compare = comparePathPiece(
          oldIter->first,
          newIter->first,
          getMount()->getCheckoutConfig()->getCaseSensitive());

      if (compare == CompareResult::BEFORE) {
        action = processCheckoutEntry(
            ctx,
            *contents,
            &*oldIter,
            nullptr,
            pendingLoads,
            wasDirectoryListModified);
        ++oldIter;
      } else if (compare == CompareResult::AFTER) {
        action = processCheckoutEntry(
            ctx,
            *contents,
            nullptr,
            &*newIter,
            pendingLoads,
            wasDirectoryListModified);
        ++newIter;
      } else {
        action = processCheckoutEntry(
            ctx,
            *contents,
            &*oldIter,
            &*newIter,
            pendingLoads,
            wasDirectoryListModified);
        ++oldIter;
        ++newIter;
      }
    }

    if (action) {
      actions.push_back(std::move(action));
    }
  }
}

unique_ptr<CheckoutAction> TreeInode::processCheckoutEntry(
    CheckoutContext* ctx,
    TreeInodeState& state,
    const Tree::value_type* oldScmEntry,
    const Tree::value_type* newScmEntry,
    vector<IncompleteInodeLoad>& pendingLoads,
    bool& wasDirectoryListModified) {
  XLOG(DBG5) << "processCheckoutEntry(" << getLogPath() << "): "
             << (oldScmEntry
                     ? oldScmEntry->second.toLogString(oldScmEntry->first)
                     : "(null)")
             << " -> "
             << (newScmEntry
                     ? newScmEntry->second.toLogString(newScmEntry->first)
                     : "(null)");
  // At most one of oldScmEntry and newScmEntry may be null.
  XDCHECK(oldScmEntry || newScmEntry);

  // If we aren't doing a force checkout, we don't need to do anything
  // for entries that are identical between the old and new source control
  // trees.
  //
  // If we are doing a force checkout we need to process unmodified entries to
  // revert them to the desired state if they were modified in the local
  // filesystem.
  if (!ctx->forceUpdate() && oldScmEntry && newScmEntry &&
      oldScmEntry->second.getType() == newScmEntry->second.getType() &&
      getObjectStore().areObjectsKnownIdentical(
          oldScmEntry->second.getHash(), newScmEntry->second.getHash())) {
    // TODO: Should we perhaps fall through anyway to report conflicts for
    // locally modified files?
    return nullptr;
  }

  // Look to see if we have a child entry with this name.
  bool contentsUpdated = false;
  const auto& name = oldScmEntry ? oldScmEntry->first : newScmEntry->first;
  auto& contents = state.entries;
  auto it = contents.find(name);
  if (it == contents.end()) {
    if (!oldScmEntry) {
      // This is a new entry being added, that did not exist in the old tree
      // and does not currently exist in the filesystem.  Go ahead and add it
      // now.
      if (!ctx->isDryRun()) {
        contentsUpdated = true;
      }
    } else if (!newScmEntry) {
      // This file exists in the old tree, but is being removed in the new
      // tree.  It has already been removed from the local filesystem, so
      // we are already in the desired state.
      //
      // We can proceed, but we still flag this as a conflict.
      ctx->addConflict(ConflictType::MISSING_REMOVED, this, oldScmEntry->first);
    } else {
      // The file was removed locally, but modified in the new tree.
      ctx->addConflict(
          ConflictType::REMOVED_MODIFIED, this, oldScmEntry->first);
      if (ctx->forceUpdate()) {
        XDCHECK(!ctx->isDryRun());
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

      auto success = invalidateChannelEntryCache(state, name, std::nullopt);
      if (success.hasValue()) {
        auto [it, inserted] = contents.emplace(
            newScmEntry->first,
            modeFromTreeEntryType(newScmEntry->second.getType()),
            getOverlay()->allocateInodeNumber(),
            newScmEntry->second.getHash());
        XDCHECK(inserted);
      } else {
        if (folly::kIsWindows) {
          if (auto* exc = success.tryGetExceptionObject<std::system_error>();
              exc && isEnotempty(*exc)) {
            XLOG(DBG6)
                << "entry was created on disk while checkout is in progress: "
                << getLogPath() << "/" << name;
            if (oldScmEntry) {
              ctx->addConflict(ConflictType::MODIFIED_MODIFIED, this, name);
            } else {
              ctx->addConflict(ConflictType::UNTRACKED_ADDED, this, name);
            }
            return nullptr;
          }
        }
        ctx->addError(this, name, success.exception());
      }
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
  // On Windows, we need to invalidate ProjectedFS on-disk state.
  if (entry.isMaterialized() ||
      getInodeMap()->isInodeRemembered(entry.getInodeNumber()) ||
      (kPreciseInodeNumberMemory && entry.isDirectory() &&
       getOverlay()->hasOverlayDir(entry.getInodeNumber()))) {
    XLOG(DBG6) << "must load child: inode=" << getNodeId() << " child=" << name;
    // This child is potentially modified (or has saved state that must be
    // updated), but is not currently loaded. Start loading it and create a
    // CheckoutAction to process it once it is loaded.
    auto inodeFuture = loadChildLocked(
        contents, name, entry, pendingLoads, ctx->getFetchContext());
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
  } else if (
      newScmEntry &&
      getObjectStore().areObjectsKnownIdentical(
          entry.getHash(), newScmEntry->second.getHash())) {
    // The inode already matches the checkout destination. So do nothing.
    return nullptr;
  } else {
    switch (getObjectStore().compareObjectsById(
        entry.getHash(), oldScmEntry->second.getHash())) {
      case ObjectComparison::Unknown: {
        // We don't know if the files are different or not. The only way to know
        // for sure is to load the inode.
        auto inodeFuture = loadChildLocked(
            contents, name, entry, pendingLoads, ctx->getFetchContext());
        return make_unique<CheckoutAction>(
            ctx, oldScmEntry, newScmEntry, std::move(inodeFuture));
      }
      case ObjectComparison::Identical:
        // We know the objects are identical, so there are no conflicts.
        // Now fall through and possibly recurse.
        break;
      case ObjectComparison::Different:
        // We know the objects are different, so report a conflict.
        conflictType = ConflictType::MODIFIED_MODIFIED;
        break;
    }
  }

  if (conflictType != ConflictType::ERROR) {
    // If this is a directory we unfortunately have to load it and recurse into
    // it just so we can accurately report the list of files with conflicts.
    if (entry.isDirectory()) {
      auto inodeFuture = loadChildLocked(
          contents, name, entry, pendingLoads, ctx->getFetchContext());
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

  // We are removing or replacing an entry - attempt to invalidate it while the
  // write lock is held and before the contents are updated.
  auto success = invalidateChannelEntryCache(state, name, oldEntryInodeNumber);
  if (success.hasException()) {
    if (folly::kIsWindows) {
      // On Windows, reads aren't being done on the inodes, but on the Trees
      // directly, when a file/directory is looked up, the dispatcher will
      // first return the data to ProjectedFS and then in the background update
      // the inodes hierarchy to ensure that the fsRefcount is set.
      //
      // Unfortunately, this means that the inode hierarchy can be slightly out
      // of date. This is one case. A recursive `grep` running concurrently
      // with checkout would populate the working copy without immediately
      // loading inodes. In that case, the invalidateChannelEntryCache will
      // fail with an ENOTEMPTY error. Let's catch this and recurse down as if
      // the directory was already loaded.
      if (auto* exc = success.tryGetExceptionObject<std::system_error>();
          exc && isEnotempty(*exc)) {
        XLOG(DBG6) << "loading child inode after invalidation failed: inode="
                   << getNodeId() << " child=" << name;
        auto inodeFuture = loadChildLocked(
            contents, name, entry, pendingLoads, ctx->getFetchContext());
        return make_unique<CheckoutAction>(
            ctx, oldScmEntry, newScmEntry, std::move(inodeFuture));
      }
    }
    ctx->addError(this, name, success.exception());
    return nullptr;
  }

  // TODO: remove entry.getInodeNumber() from both the overlay and the
  // InodeTable.  Or at least verify that it's already done in a test.
  //
  // This logic could potentially be unified with TreeInode::tryRemoveChild
  // and TreeInode::checkoutUpdateEntry.
  contents.erase(it);
  if (newScmEntry) {
    contents.emplace(
        newScmEntry->first,
        modeFromTreeEntryType(newScmEntry->second.getType()),
        getOverlay()->allocateInodeNumber(),
        newScmEntry->second.getHash());
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
    getOverlay()->recursivelyRemoveOverlayDir(oldEntryInodeNumber);
  }

  // TODO: contents have changed: we probably should propagate
  // this information up to our caller so it can mark us
  // materialized if necessary.

  return nullptr;
}

namespace {
/**
 * Get this Inode's name.
 */
PathComponent getInodeName(CheckoutContext* ctx, const InodePtr& inode) {
  return inode->getLocationInfo(ctx->renameLock()).name;
}
} // namespace

Future<InvalidationRequired> TreeInode::checkoutUpdateEntry(
    CheckoutContext* ctx,
    PathComponentPiece name,
    InodePtr inode,
    std::shared_ptr<const Tree> oldTree,
    std::shared_ptr<const Tree> newTree,
    const std::optional<Tree::value_type>& newScmEntry) {
  auto treeInode = inode.asTreePtrOrNull();
  if (!treeInode) {
    // If the target of the update is not a directory, then we know we do not
    // need to recurse into it, looking for more conflicts, so we can exit here.
    if (ctx->isDryRun()) {
      return InvalidationRequired::No;
    }

    std::optional<PathComponent> inodeName;
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

      // Tell the OS to invalidate its cache for this entry. For case
      // insensitive mounts, we need to invalidate the current name, hence
      // using it->first instead of name.
      auto success = invalidateChannelEntryCache(
          *contents, it->first, it->second.getInodeNumber());
      if (success.hasException()) {
        if (folly::kIsWindows) {
          if (auto* exc = success.tryGetExceptionObject<std::system_error>();
              exc && isEnotempty(*exc)) {
            XLOG(DBG6) << "entry changed on disk from a file to a "
                       << "non-empty directory while checkout is in progress: "
                       << inode->getLogPath();
            if (newScmEntry) {
              ctx->addConflict(
                  ConflictType::MODIFIED_MODIFIED, this, it->first);
            } else {
              ctx->addConflict(ConflictType::MODIFIED_REMOVED, this, it->first);
            }
            return InvalidationRequired::No;
          }
        }
        ctx->addError(this, it->first, success.exception());
        return InvalidationRequired::No;
      }

      // This is a file, so we can simply unlink it, and replace/remove the
      // entry as desired.
      deletedInode = inode->markUnlinked(this, it->first, ctx->renameLock());
      contents->entries.erase(it);

      if (newScmEntry) {
        auto [it, inserted] = contents->entries.emplace(
            newScmEntry->first,
            modeFromTreeEntryType(newScmEntry->second.getType()),
            getOverlay()->allocateInodeNumber(),
            newScmEntry->second.getHash());
        XDCHECK(inserted);
      }
    }

    // We don't save our own overlay data right now:
    // we'll wait to do that until the checkout operation finishes touching all
    // of our children in checkout().
    return InvalidationRequired::Yes;
  }

  // If we are going from a directory to a directory, all we need to do
  // is call checkout().
  if (newTree) {
    XCHECK(newScmEntry.has_value());
    XCHECK(newScmEntry->second.isTree());

    if (getMount()->getCheckoutConfig()->getCaseSensitive() ==
            CaseSensitivity::Insensitive &&
        newScmEntry->first != getInodeName(ctx, treeInode)) {
      // For case insensitive mount, the name of the new and old entries might
      // differ in casing. In that case, we want to fallthrough to the case
      // below to force the old name to be removed and then re-added with its
      // new name.
    } else {
      // TODO: SCM entries today have limited file modes. We have simplified
      // checkout to only handle directory, symlink, file, executable file.
      // If we were to support full file permissions being updated via
      // checkout, we would need to do that in or after the checkout operation.
      // NFS invalidation is a hack, and a hack that relies on directory
      // permissions never being changed during the checkout operation. We
      // would need to be more clever with our invalidation hack for NFS if we
      // supported changing permissions on checkout.
      return treeInode->checkout(ctx, std::move(oldTree), std::move(newTree))
          .thenValue([](folly::Unit) { return InvalidationRequired::No; });
    }
  }

  // We need to remove this directory (and possibly replace it with a file).
  // First we have to recursively unlink everything inside the directory.
  // Fortunately, calling checkout() with an empty destination tree does
  // exactly what we want.
  return treeInode->checkout(ctx, std::move(oldTree), nullptr)
      .thenValue(
          [ctx,
           newTree = std::move(newTree),
           parentInode = inodePtrFromThis(),
           treeInode,
           newScmEntry](auto&&) mutable -> folly::Future<InvalidationRequired> {
            if (ctx->isDryRun()) {
              // If this is a dry run, simply report conflicts and don't update
              // or invalidate the inode.
              return InvalidationRequired::No;
            }

            const auto& name = getInodeName(ctx, treeInode);

            // Now we can attempt to delete treeInode!
            // The ordering of invalidateChannelEntryCache and tryRemoveChild
            // is important on Windows here. We need to attempt to clear the
            // filesystem data before we delete the inode because the kernel is
            // the source of truth. If invalidating fails, we do not want to
            // actually delete the inode from eden's state. Note: On NFS and
            // FUSE, EdenFS is the source of truth. So in theory one might want
            // to change eden first and then FUSE or NFS. On FUSE this doesn't
            // really matter, if we invalidated fuse and then removing in eden
            // fails, its fine we did the extra invalidation. On NFS our
            // invalidation relies on data changing in EdenFS and the kernel
            // noticing and clearing it's own caches. So we would really want
            // tryRemoveChild to happen first. But thankfully
            // invalidateChannelEntryCache doesn't do anything on NFS anyways.
            // so it does not matter these are out of order.

            if (parentInode
                    ->invalidateChannelEntryCache(
                        *parentInode->contents_.wlock(),
                        name,
                        treeInode->getNodeId())
                    .hasException()) {
              if (newTree) {
                XCHECK_EQ(
                    parentInode->getMount()
                        ->getCheckoutConfig()
                        ->getCaseSensitive(),
                    CaseSensitivity::Insensitive);
                XCHECK_NE(newScmEntry->first, name);
                // Because invalidateChannelEntryCache can only fail on Windows
                // and PrjFS, the mount must be case-insensitive. Moreover,
                // newScmEntry->first and name are different, so the case of
                // the directory changed. Unfortunately, we couldn't remove the
                // directory from the disk, and thus we are unable to actually
                // change the case. This can be due to the directory containing
                // an untracked file for instance. We can however fallback to
                // updating the directory itself to the newTree. This behavior
                // is consistent with vanilla Mercurial.
                return treeInode->checkout(ctx, nullptr, std::move(newTree))
                    .thenValue(
                        [](folly::Unit) { return InvalidationRequired::No; });
              } else {
                ctx->addConflict(
                    ConflictType::DIRECTORY_NOT_EMPTY, treeInode.get());
                return InvalidationRequired::No;
              }
            }

            if (parentInode->tryRemoveChild(
                    ctx->renameLock(),
                    name,
                    treeInode,
                    InvalidationRequired::No) != 0) {
              ctx->addConflict(
                  ConflictType::DIRECTORY_NOT_EMPTY, treeInode.get());
              // Since we've invalidated the entry, even if this fails we need
              // to make sure the directory is also invalidated, fallthrough.
            }

            // If the entry does not exist at the new commit we can stop here.
            // no need to add anything back to our parent's contents.
            if (!newScmEntry) {
              return InvalidationRequired::Yes;
            }

            // On case insensitive mounts, a change of casing would lead to a
            // removal of this TreeInode followed by the insertion of the
            // different cased TreeInode.
            if (newScmEntry->second.isTree()) {
              XDCHECK_EQ(
                  parentInode->getMount()
                      ->getCheckoutConfig()
                      ->getCaseSensitive(),
                  CaseSensitivity::Insensitive);
            }

            bool inserted;
            {
              auto contents = parentInode->contents_.wlock();
              auto ret = contents->entries.emplace(
                  newScmEntry->first,
                  modeFromTreeEntryType(newScmEntry->second.getType()),
                  parentInode->getOverlay()->allocateInodeNumber(),
                  newScmEntry->second.getHash());
              inserted = ret.second;
            }

            if (!inserted) {
              // Hmm.  Someone else already created a new entry in
              // this location before we had a chance to add our new
              // entry.  We don't block new file or directory
              // creations during a checkout operation, so this is
              // possible.  Just report an error in this case.
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

            // Make sure that we invalidate the directory in
            // TreeInode::checkout.
            return InvalidationRequired::Yes;
          });
}

#ifdef _WIN32
namespace {
/**
 * Test if the passed in InodeNumber is known by the the InodeMap.
 */
bool needDecFsRefcount(InodeMap& inodeMap, InodeNumber ino) {
  return inodeMap.isInodeLoadedOrRemembered(ino);
}
} // namespace
#endif

folly::Try<folly::Unit> TreeInode::invalidateChannelEntryCache(
    TreeInodeState&,
    PathComponentPiece name,
    FOLLY_MAYBE_UNUSED std::optional<InodeNumber> ino) {
#ifndef _WIN32
  if (auto* fuseChannel = getMount()->getFuseChannel()) {
    fuseChannel->invalidateEntry(getNodeId(), name);
  }
  // For NFS, the entry cache is flushed when the directory mtime is changed.
  // Directly invalidating an entry is not possible.
#else
  if (auto* fsChannel = getMount()->getPrjfsChannel()) {
    const auto path = getPath();
    if (path.has_value()) {
      // Try to remove the file first, and then call decFsRefcount if needed.
      // When no inode number is passed in, we still need to invalidate the
      // ProjectedFS file, as tombstones are a special kind of placeholder
      // that EdenFS doesn't have inodes for.
      auto ret = fsChannel->removeCachedFile(path.value() + name);
      if (ret.hasValue()) {
        auto& inodeMap = *getInodeMap();
        if (ino && needDecFsRefcount(inodeMap, *ino)) {
          // At this point, the file is now virtual, that is no placeholder or
          // full file are present on disk. If at this point, the file is being
          // looked up, EdenFS will service the lookup in
          // PrjfsDispatcherImpl::lookup, and then try to increment the
          // refcount. The refcount increment is guarantee to happen after the
          // decrement below due to the increment needing to traverse the inode
          // hierarchy and thus acquiring the content lock. The same content
          // lock that is held in this function.
          getInodeMap()->decFsRefcount(*ino);
        }
      }
      return ret;
    }
  }
#endif

  return folly::Try<folly::Unit>{folly::unit};
}

ImmediateFuture<folly::Unit> TreeInode::invalidateChannelDirCache(
    TreeInodeState& state) {
#ifndef _WIN32
  if (auto* fuseChannel = getMount()->getFuseChannel()) {
    // FUSE_NOTIFY_INVAL_ENTRY is the appropriate invalidation function
    // when an entry is removed or modified. But when new entries are
    // added, the inode itself must be invalidated.
    fuseChannel->invalidateInode(getNodeId(), 0, 0);
  } else if (auto* nfsdChannel = getMount()->getNfsdChannel()) {
    const auto path = getPath();
    if (path.has_value()) {
      auto mode = getMetadataLocked(state.entries).mode;
      nfsdChannel->invalidate(getMount()->getPath() + *path, mode);
    }
  }
#else
  (void)state;
  if (auto* fsChannel = getMount()->getPrjfsChannel()) {
    const auto path = getPath();
    if (path.has_value()) {
      // Invalidation may block, thus in order to not starve the server thread
      // pool during checkout let's move it to a separate thread.
      return ImmediateFuture{folly::via(
                                 getMount()->getInvalidationThreadPool().get(),
                                 [fsChannel, path = std::move(path).value()]() {
                                   // Don't call decFsRefcount here as we're
                                   // adding a placeholder, and thus this
                                   // TreeInode need to be kept in the InodeMap
                                   // until invalidateChannelEntryCache is
                                   // called on it.
                                   return fsChannel->addDirectoryPlaceholder(
                                       path);
                                 })
                                 .semi()};
    }
  }
#endif

  return folly::unit;
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
  {
    auto contents = contents_.wlock();

    // Check to see if we need to be materialized or not.
    //
    // If we can confirm that we are identical to the source control Tree we
    // do not need to be materialized.
    auto tryToDematerialize = [&]() -> std::optional<ObjectId> {
      // If the new tree does not exist in source control, we must be
      // materialized, since there is no source control Tree to refer to.
      if (!tree) {
        return std::nullopt;
      }

      // If we have a different number of entries we must be different from
      // the Tree, and therefore must be materialized.
      if (tree->size() != contents->entries.size()) {
        return std::nullopt;
      }

      // This code relies on the fact that our contents->entries PathMap sorts
      // paths in the same order as Tree's entry list.
      auto inodeIter = contents->entries.begin();
      auto scmIter = tree->begin();
      for (; scmIter != tree->end(); ++inodeIter, ++scmIter) {
        // If any of our children are materialized, we need to be materialized
        // too to record the fact that we have materialized children.
        //
        // If our children are materialized this means they are likely
        // different from the new source control state.  (This is not a 100%
        // guarantee though, as writes may still be happening concurrently to
        // the checkout operation.)  Even if the child is still identical to
        // its source control state we still want to make sure we are
        // materialized if the child is.
        if (inodeIter->second.isMaterialized()) {
          return std::nullopt;
        }

        // TODO: This needs to compare filenames too.

        // If the child is not materialized, it is the same as some source
        // control object.  However, if it isn't the same as the object in our
        // Tree, we have to materialize ourself.
        switch (getObjectStore().compareObjectsById(
            inodeIter->second.getHash(), scmIter->second.getHash())) {
          case ObjectComparison::Unknown:
            // Assume the child is different, and leave materialized.
            return std::nullopt;
          case ObjectComparison::Identical:
            // The IDs refer to the same object, so we can dematerialize. Even
            // if the IDs don't match exactly, we'll silently migrate to the
            // new ID scheme here.
            break;
          case ObjectComparison::Different:
            // The objects differ, so we can't dematerialize.
            return std::nullopt;
        }
      }

      // TODO: This check should be removed and instead a
      // std::optional<ObjectId> should be passed to
      // TreeInode::saveoverlayPostCheckout. The issue is that setPathRootId
      // synthesizes a fake Tree and then calls checkout, which might notice
      // that the previous fake Tree and the current fake Tree have the same
      // hash, which will incorrectly dematerialized this inode. The fake hash
      // cannot be reconstituted from the backing store, so this makes the
      // directory structure unreadable. The correct long-term fix is to
      // remove getHash() from Tree and pass around ObjectIds explicitly if
      // known.
      if (tree->getHash().size() == 0) {
        return std::nullopt;
      }

      // If we're still here we are identical to the source control Tree.
      // We can be dematerialized and marked identical to the input Tree.
      return tree->getHash();
    };

    auto oldHash = contents->treeHash;
    auto newHash = tryToDematerialize();
    contents->treeHash = newHash;
    isMaterialized = contents->isMaterialized();
    // If our tree hash changed, even if it references the same contents, we
    // must tell the parent so it can update its hash. Therefore, don't use
    // BackingStore::areObjectsKnownIdentical here.
    if (oldHash.has_value() && newHash.has_value()) {
      stateChanged = !oldHash->bytesEqual(*newHash);
    } else if (!oldHash.has_value() && !contents->treeHash.has_value()) {
      stateChanged = false;
    } else {
      stateChanged = true;
    }

    XLOG(DBG4) << "saveOverlayPostCheckout(" << getLogPath() << ", " << tree
               << "): oldHash="
               << (oldHash ? oldHash.value().toLogString() : "none")
               << " newHash="
               << (contents->treeHash ? contents->treeHash.value().toLogString()
                                      : "none")
               << " isMaterialized=" << isMaterialized;

    // Update the overlay to include the new entries, even if dematerialized.
    saveOverlayDir(contents->entries);
  }

  if (stateChanged) {
    // If our state changed, tell our parent.
    //
    // TODO: Currently we end up writing out overlay data for TreeInodes
    // pretty often during the checkout process.  Each time a child entry is
    // processed we will likely end up rewriting data for it's parent
    // TreeInode, and then once all children are processed we do another pass
    // through here in saveOverlayPostCheckout() and possibly write it out
    // again.
    //
    // It would be nicer if we could only save the data for each TreeInode
    // once.  The downside of this is that the on-disk overlay state would be
    // potentially inconsistent until the checkout completes.  There may be
    // periods of time where a parent directory says the child is materialized
    // when the child has decided to be dematerialized.  This would cause
    // problems when we tried to load the overlay data later.  If we update
    // the code to be able to handle this somehow then maybe we could avoid
    // doing all of the intermediate updates to the parent as we process each
    // child entry.
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

folly::Future<InodePtr> TreeInode::loadChildLocked(
    DirContents& /* contents */,
    PathComponentPiece name,
    DirEntry& entry,
    std::vector<IncompleteInodeLoad>& pendingLoads,
    const ObjectFetchContextPtr& fetchContext) {
  XDCHECK(!entry.getInode());

  folly::Promise<InodePtr> promise;
  auto future = promise.getFuture();
  auto childNumber = entry.getInodeNumber();
  bool startLoad = getInodeMap()->startLoadingChildIfNotLoading(
      this, name, childNumber, entry.getInitialMode(), std::move(promise));
  if (startLoad) {
    auto loadFuture = startLoadingInodeNoThrow(entry, name, fetchContext);
    pendingLoads.emplace_back(
        this, std::move(loadFuture), name, entry.getInodeNumber());
  }

  return future;
}

namespace {
/**
 * WARNING: predicate is called while the InodeMap and TreeInode contents
 * locks are held.
 */
template <typename Recurse, typename Predicate>
size_t unloadChildrenIf(
    TreeInode* const self,
    InodeMap* const inodeMap,
    std::vector<TreeInodePtr>& treeChildren,
    Recurse&& recurse,
    Predicate&& predicate) {
  size_t unloadCount = 0;

  // Recurse into children here. Children hold strong references to their
  // parent trees, so unloading children can cause the parent to become
  // unreferenced.
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

      // Check isPtrAcquireCountZero() first. It's a single load instruction
      // on x86 and if the predicate calls getFuseRefcount(), it will assert
      // if isPtrAcquireCountZero() is false.
      if (entryInode->isPtrAcquireCountZero() && predicate(entryInode)) {
        // If it's a tree and it has a loaded child, its refcount will never
        // be zero because the child holds a reference to its parent.

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
  // Outside of the locks, deallocate all of the inodes scheduled to be
  // deleted.
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

      // This has the side effect of incrementing the reference counts of
      // all of the children. When that goes back to zero,
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

size_t TreeInode::unloadChildrenUnreferencedByFs() {
  auto treeChildren = getTreeChildren(this);
  return unloadChildrenIf(
      this,
      getInodeMap(),
      treeChildren,
      [](TreeInode& child) { return child.unloadChildrenUnreferencedByFs(); },
      [](InodeBase* child) { return child->getFsRefcount() == 0; });
}

namespace {
ImmediateFuture<std::vector<TreeInodePtr>> getLoadedOrRememberedTreeChildren(
    TreeInode* self,
    InodeMap* const inodeMap,
    const ObjectFetchContextPtr& context) {
  std::vector<ImmediateFuture<TreeInodePtr>> res;
  std::vector<PathComponent> toLoad;
  {
    auto contents = self->getContents().rlock();
    for (auto& entry : contents->entries) {
      if (!entry.second.isDirectory()) {
        continue;
      }

      if (auto treePtr = entry.second.asTreePtrOrNull()) {
        res.emplace_back(std::move(treePtr));
        continue;
      }

      auto inodeNumber = entry.second.getInodeNumber();
      // In invalidateChildrenNotMaterialized we want to walk all the directory
      // inodes that are present on disk so we can have a chance to invalidate
      // them. Since inodes can be unloaded but still have an fs refcount set,
      // we need to make sure to load them so we can crawl them.
      if (inodeMap->isInodeRemembered(inodeNumber)) {
        toLoad.push_back(entry.first);
      }
    }
  }

  // TODO(xavierd): We could use VirtualInode here to avoid loading inodes
  // unnecessarily.
  for (auto& name : toLoad) {
    res.push_back(self->getOrLoadChildTree(name, context));
  }
  return collectAllSafe(std::move(res));
}
} // namespace

ImmediateFuture<uint64_t> TreeInode::invalidateChildrenNotMaterialized(
    std::chrono::system_clock::time_point cutoff,
    const ObjectFetchContextPtr& context) {
  return getLoadedOrRememberedTreeChildren(this, getInodeMap(), context)
      .thenValue([context = context.copy(),
                  cutoff](std::vector<TreeInodePtr> treeChildren) {
        std::vector<ImmediateFuture<uint64_t>> futures;

        for (auto& tree : treeChildren) {
          futures.push_back(
              tree->invalidateChildrenNotMaterialized(cutoff, context));
        }

        return collectAllSafe(std::move(futures));
      })
      .thenValue([self = inodePtrFromThis(),
                  cutoff](const std::vector<uint64_t>& invalidatedCounts) {
        uint64_t numInvalidated = 0;
        for (auto invalidated : invalidatedCounts) {
          numInvalidated += invalidated;
        }

        std::vector<InodePtr> deletedInodes;
        {
          AbsolutePath selfPath;
          if (auto path = self->getPath()) {
            selfPath = self->getMount()->getPath() + path.value();
          } else {
            // This directory was removed, no need to do anything.
            return numInvalidated;
          }

          auto* inodeMap = self->getInodeMap();
          auto contents = self->contents_.wlock();
          for (auto& entry : contents->entries) {
            if (entry.second.isMaterialized()) {
              continue;
            }

            if (auto inode = entry.second.getInodePtr()) {
              deletedInodes.push_back(std::move(inode));
            }

            auto inodeNumber = entry.second.getInodeNumber();
            if (!inodeMap->isInodeLoadedOrRemembered(inodeNumber)) {
              continue;
            }

#ifdef _WIN32
            // Let's focus only on files as directories will get their atime
            // updated when we query the atime of the files contained in it.
            if (!entry.second.isDirectory()) {
              auto entryPath = selfPath + entry.first;
              auto wEntryPath = entryPath.wide();
              struct __stat64 buf;

              // TODO: If the file isn't on disk this will lay a placeholder on
              // disk and at the same time force it to not be invalidated due
              // to its atime being newer than the cutoff.
              if (_wstat64(wEntryPath.c_str(), &buf) < 0) {
                continue;
              }

              auto atime = std::chrono::system_clock::from_time_t(buf.st_atime);
              if (atime > cutoff) {
                // That file has been touched too recently, continue.
                continue;
              }
            }
#else
            (void)cutoff;

        // TODO(xavierd): read the atime from the InodeMetadata table.
#endif

            // TODO: In the case where the file becomes materialized on disk
            // now, invalidateChannelEntryCache will happily remove it, leading
            // to a potential loss of user data. To avoid this, we could try
            // not passing PRJ_UPDATE_ALLOW_DIRTY_DATA and dealing with the
            // side effects to close that race.

            // Here, we rely on invalidateChannelEntryCache failing for
            // non-empty directories to guarantee that we're not losing user
            // data in the case where a user writes a file in a directory that
            // we're attempting to invalidate. For directories with not
            // invalidated childrens due to being read too recently, we also
            // rely on invalidateChannelEntryCache failing.
            auto invalidateResult = self->invalidateChannelEntryCache(
                *contents, entry.first, inodeNumber);
            if (invalidateResult.hasException()) {
              XLOG(DBG5) << "Couldn't invalidate: " << self->getLogPath() << "/"
                         << entry.first << ": " << invalidateResult.exception();
            } else {
              numInvalidated++;
            }
          }
        }

        return numInvalidated;
      });
}

void TreeInode::updateAtime() {
  auto lock = contents_.wlock();
  InodeBaseMetadata::updateAtimeLocked(lock->entries);
}

void TreeInode::forceMetadataUpdate() {
  auto contents = contents_.wlock();
  InodeBaseMetadata::updateMtimeAndCtimeLocked(contents->entries, getNow());
}

#ifndef _WIN32
ImmediateFuture<folly::Unit> TreeInode::ensureMaterialized(
    const ObjectFetchContextPtr& fetchContext,
    bool followSymlink) {
  std::vector<ImmediateFuture<folly::Unit>> childFutures;
  std::vector<PathComponent> names;
  {
    auto contents = contents_.rlock();
    names.reserve(contents->entries.size());
    for (auto& entry : contents->entries) {
      names.emplace_back(entry.first);
    }
  }

  childFutures.reserve(names.size());
  for (auto& name : names) {
    childFutures.emplace_back(
        getOrLoadChild(name, fetchContext)
            .thenValue([fetchContext = fetchContext.copy(),
                        followSymlink](InodePtr inodePtr) {
              return inodePtr->ensureMaterialized(fetchContext, followSymlink);
            }));
  }

  return collectAll(std::move(childFutures)).unit();
}
#endif

#ifndef _WIN32
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

  // We no longer need pointers to the child inodes - release them. Beware
  // that this may deallocate inode instances for the children and clear them
  // from InodeMap and contents table as a natural side effect of their
  // refcounts going to zero.
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

InodeMetadata TreeInode::getMetadata() const {
  auto lock = contents_.rlock();
  return getMetadataLocked(lock->entries);
}

InodeMetadata TreeInode::getMetadataLocked(const DirContents&) const {
  return getMount()->getInodeMetadataTable()->getOrThrow(getNodeId());
}

void TreeInode::prefetch(const ObjectFetchContextPtr& context) {
  bool expected = false;
  if (!prefetched_.compare_exchange_strong(expected, true)) {
    return;
  }
  // Blob metadata will already be prefetched when this
  // tree is first fetched. This could beat the metadata
  // prefetch to the punch and cause a full blob fetch.
  // So when metadata prefetching is turned on we can
  // just skip this.
  auto config = getMount()->getServerState()->getEdenConfig();
  if (config->useAuxMetadata.getValue()) {
    XLOG(DBG4) << "skipping prefetch for " << getLogPath()
               << ": metadata prefetching is turned on in the backing store";
    return;
  }
  auto prefetchLease =
      getMount()->tryStartTreePrefetch(inodePtrFromThis(), *context);
  if (!prefetchLease) {
    XLOG(DBG3) << "skipping prefetch for " << getLogPath()
               << ": too many prefetches already in progress";
    prefetched_.store(false);
    return;
  }
  XLOG(DBG4) << "starting prefetch for " << getLogPath();

  folly::via(
      getMount()->getServerThreadPool().get(),
      [lease = std::move(*prefetchLease)]() mutable {
        // prefetch() is called by readdir, under the assumption that a series
        // of stat calls on its entries will follow. (e.g. `ls -l` or `find
        // -ls`). To optimize that common situation, load trees and blob
        // metadata in parallel here.

        std::vector<IncompleteInodeLoad> pendingLoads;
        std::vector<Future<Unit>> inodeFutures;
        // The aliveness of this context is guaranteed by the `.thenTry`
        // capture at the end of this lambda
        auto& context = lease.getContext();

        {
          auto contents = lease.getTreeInode()->contents_.wlock();

          for (auto& [name, entry] : contents->entries) {
            if (entry.getInode()) {
              // Already loaded
              continue;
            }

            // Userspace will commonly issue a readdir() followed by a series
            // of stat()s. In FUSE, that translates into readdir() and then
            // lookup(), which returns the same information as a stat(),
            // including the number of directory entries or number of bytes in
            // a file. Perform those operations here by loading inodes, trees,
            // and blob sizes.
            inodeFutures.emplace_back(
                lease.getTreeInode()
                    ->loadChildLocked(
                        contents->entries, name, entry, pendingLoads, context)
                    .thenValue([context = context.copy()](InodePtr inode) {
                      return inode->stat(context).semi();
                    })
                    .unit());
          }
        }

        // Hook up the pending load futures to properly complete the loading
        // process then the futures are ready.  We can only do this after
        // releasing the contents_ lock.
        for (auto& load : pendingLoads) {
          load.finish();
        }

        return folly::collectAllUnsafe(inodeFutures)
            .thenTry([lease = std::move(lease)](auto&&) {
              XLOG(DBG4) << "finished prefetch for "
                         << lease.getTreeInode()->getLogPath();
            });
      });
}

ImmediateFuture<struct stat> TreeInode::setattr(
    const DesiredMetadata& desired,
    const ObjectFetchContextPtr& /*fetchContext*/) {
  struct stat result(getMount()->initStatData());
  result.st_ino = getNodeId().get();

  // Ideally, we would like to take the lock once for this function
  // call, but we cannot hold the lock while we materialize, so we
  // have to take the lock twice.
  {
    auto contents = contents_.wlock();
    auto existing = getMetadataLocked(contents->entries);

    if (existing.shouldShortCircuitMetadataUpdate(desired)) {
      existing.applyToStat(result);
      XLOG(DBG7) << "Skipping materialization because setattr is a noop";
      return result;
    }
  }
  // The attributes actually changed so we need to mark this directory as
  // modified.
  materialize();

  // We do not have size field for directories and currently TreeInode does
  // not have any field like FileInode::state_::mode to set the mode. May be
  // in the future if needed we can add a mode Field to TreeInode::contents_
  // but for now we are simply setting the mode to (S_IFDIR | 0755).

  // Set timeStamps, mode in the result.
  auto contents = contents_.wlock();
  auto metadata = getMount()->getInodeMetadataTable()->modifyOrThrow(
      getNodeId(),
      [&](auto& metadata) { metadata.updateFromDesired(getClock(), desired); });
  metadata.applyToStat(result);

  // Update Journal
  updateJournal();
  return result;
}

ImmediateFuture<std::vector<std::string>> TreeInode::listxattr() {
  return std::vector<std::string>{};
}
ImmediateFuture<std::string> TreeInode::getxattr(
    folly::StringPiece /*name*/,
    const ObjectFetchContextPtr& /*context*/) {
  return makeImmediateFuture<std::string>(
      InodeError(kENOATTR, inodePtrFromThis()));
}
#endif

} // namespace facebook::eden
