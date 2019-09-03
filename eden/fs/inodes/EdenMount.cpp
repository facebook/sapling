/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/inodes/EdenMount.h"

#include <boost/filesystem.hpp>
#include <folly/ExceptionWrapper.h>
#include <folly/FBString.h>
#include <folly/File.h>
#include <folly/chrono/Conv.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/Logger.h>
#include <folly/logging/xlog.h>
#include <folly/system/ThreadName.h>
#include <gflags/gflags.h>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMountError.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/InodeTable.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TopLevelIgnores.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/service/PrettyPrinters.h"
#include "eden/fs/store/BlobAccess.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/FaultInjector.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

using folly::Future;
using folly::makeFuture;
using folly::setThreadName;
using folly::StringPiece;
using folly::to;
using folly::Unit;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::unique_ptr;
using std::vector;
using std::chrono::system_clock;

DEFINE_int32(fuseNumThreads, 16, "how many fuse dispatcher threads to spawn");

namespace facebook {
namespace eden {

namespace {
// We used to play tricks and hard link the .eden directory
// into every tree, but the linux kernel doesn't seem to like
// hard linking directories.  Now we create a symlink that resolves
// to the .eden directory inode in the root.
// The name of that symlink is `this-dir`:
// .eden/this-dir -> /abs/path/to/mount/.eden
const PathComponentPiece kDotEdenSymlinkName{"this-dir"_pc};
} // namespace

/**
 * Helper for computing unclean paths when changing parents
 *
 * This InodeDiffCallback instance is used to compute the set
 * of unclean files before and after actions that change the
 * current commit hash of the mount point.
 */
class EdenMount::JournalDiffCallback : public InodeDiffCallback {
 public:
  explicit JournalDiffCallback()
      : data_{folly::in_place, std::unordered_set<RelativePath>()} {}

  void ignoredFile(RelativePathPiece) override {}

  void untrackedFile(RelativePathPiece) override {}

  void removedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    data_.wlock()->uncleanPaths.insert(path.copy());
  }

  void modifiedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    data_.wlock()->uncleanPaths.insert(path.copy());
  }

  void diffError(RelativePathPiece path, const folly::exception_wrapper& ew)
      override {
    // TODO: figure out what we should do to notify the user, if anything.
    // perhaps we should just add this path to the list of unclean files?
    XLOG(WARNING) << "error computing journal diff data for " << path << ": "
                  << folly::exceptionStr(ew);
  }

  FOLLY_NODISCARD Future<folly::Unit> performDiff(
      EdenMount* mount,
      TreeInodePtr rootInode,
      std::shared_ptr<const Tree> rootTree) {
    auto diffContext = mount->createDiffContext(this, /* listIgnored */ false);
    auto rawContext = diffContext.get();

    return rootInode
        ->diff(
            rawContext,
            RelativePathPiece{},
            std::move(rootTree),
            rawContext->getToplevelIgnore(),
            false)
        .ensure([diffContext = std::move(diffContext)]() {});
  }

  /** moves the Unclean Path information out of this diff callback instance,
   * rendering it invalid */
  std::unordered_set<RelativePath> stealUncleanPaths() {
    std::unordered_set<RelativePath> result;
    std::swap(result, data_.wlock()->uncleanPaths);

    return result;
  }

 private:
  struct Data {
    explicit Data(std::unordered_set<RelativePath>&& unclean)
        : uncleanPaths(std::move(unclean)) {}

    std::unordered_set<RelativePath> uncleanPaths;
  };
  folly::Synchronized<Data> data_;
};

constexpr int EdenMount::kMaxSymlinkChainDepth;
static constexpr folly::StringPiece kEdenStracePrefix = "eden.strace.";

// We compute this when the process is initialized, but stash a copy
// in each EdenMount.  We may in the future manage to propagate enough
// state across upgrades or restarts that we can preserve this, but
// as implemented today, a process restart will invalidate any cached
// mountGeneration that a client may be holding on to.
// We take the bottom 16-bits of the pid and 32-bits of the current
// time and shift them up, leaving 16 bits for a mount point generation
// number.
static const uint64_t globalProcessGeneration =
    (uint64_t(getpid()) << 48) | (uint64_t(time(nullptr)) << 16);

// Each time we create an EdenMount we bump this up and OR it together
// with the globalProcessGeneration to come up with a generation number
// for a given mount instance.
static std::atomic<uint16_t> mountGeneration{0};

std::shared_ptr<EdenMount> EdenMount::create(
    std::unique_ptr<CheckoutConfig> config,
    std::shared_ptr<ObjectStore> objectStore,
    std::shared_ptr<BlobCache> blobCache,
    std::shared_ptr<ServerState> serverState,
    std::unique_ptr<Journal> journal) {
  return std::shared_ptr<EdenMount>{new EdenMount{std::move(config),
                                                  std::move(objectStore),
                                                  std::move(blobCache),
                                                  std::move(serverState),
                                                  std::move(journal)},
                                    EdenMountDeleter{}};
}

EdenMount::EdenMount(
    std::unique_ptr<CheckoutConfig> config,
    std::shared_ptr<ObjectStore> objectStore,
    std::shared_ptr<BlobCache> blobCache,
    std::shared_ptr<ServerState> serverState,
    std::unique_ptr<Journal> journal)
    : config_{std::move(config)},
      serverState_{std::move(serverState)},
      inodeMap_{new InodeMap(this)},
      dispatcher_{new EdenDispatcher(this)},
      objectStore_{std::move(objectStore)},
      blobCache_{std::move(blobCache)},
      blobAccess_{objectStore_, blobCache_},
      overlay_{std::make_unique<Overlay>(config_->getOverlayPath())},
      overlayFileAccess_{overlay_.get()},
      bindMounts_{config_->getBindMounts()},
      journal_{std::move(journal)},
      mountGeneration_{globalProcessGeneration | ++mountGeneration},
      straceLogger_{kEdenStracePrefix.str() + config_->getMountPath().value()},
      lastCheckoutTime_{serverState_->getClock()->getRealtime()},
      owner_{Owner{getuid(), getgid()}},
      clock_{serverState_->getClock()} {}

folly::Future<folly::Unit> EdenMount::initialize(
    const std::optional<SerializedInodeMap>& takeover) {
  transitionState(State::UNINITIALIZED, State::INITIALIZING);

  return serverState_->getFaultInjector()
      .checkAsync("mount", getPath().stringPiece())
      .via(serverState_->getThreadPool().get())
      .thenValue([this](auto&&) {
        auto parents = config_->getParentCommits();
        parentInfo_.wlock()->parents.setParents(parents);

        // Record the transition from no snapshot to the current snapshot in
        // the journal.  This also sets things up so that we can carry the
        // snapshot id forward through subsequent journal entries.
        journal_->recordHashUpdate(parents.parent1());

        // Initialize the overlay.
        // This must be performed before we do any operations that may allocate
        // inode numbers, including creating the root TreeInode.
        return overlay_->initialize().deferValue(
            [parents](auto&&) { return parents; });
      })
      .thenValue(
          [this](ParentCommits&& parents) { return createRootInode(parents); })
      .thenValue([this, takeover](TreeInodePtr initTreeNode) {
        if (takeover) {
          inodeMap_->initializeFromTakeover(std::move(initTreeNode), *takeover);
        } else {
          inodeMap_->initialize(std::move(initTreeNode));
        }

        // TODO: It would be nice if the .eden inode was created before
        // allocating inode numbers for the Tree's entries. This would give the
        // .eden directory inode number 2.
        return setupDotEden(getRootInode());
      })
      .thenTry([this](auto&& result) {
        if (result.hasException()) {
          transitionState(State::INITIALIZING, State::INIT_ERROR);
        } else {
          transitionState(State::INITIALIZING, State::INITIALIZED);
        }
        return std::move(result);
      });
}

folly::Future<TreeInodePtr> EdenMount::createRootInode(
    const ParentCommits& parentCommits) {
  // Load the overlay, if present.
  auto rootOverlayDir = overlay_->loadOverlayDir(kRootNodeId);
  if (rootOverlayDir) {
    // No hash is necessary because the root is always materialized.
    return TreeInodePtr::makeNew(
        this, std::move(*rootOverlayDir), std::nullopt);
  }
  return objectStore_->getTreeForCommit(parentCommits.parent1())
      .thenValue([this](std::shared_ptr<const Tree> tree) {
        return TreeInodePtr::makeNew(this, std::move(tree));
      });
}

folly::Future<folly::Unit> EdenMount::setupDotEden(TreeInodePtr root) {
  auto createDotEdenSymlink = [this](TreeInodePtr dotEdenInode) {
    auto edenSymlink = dotEdenInode->symlink(
        kDotEdenSymlinkName,
        (config_->getMountPath() + PathComponentPiece{kDotEdenName})
            .stringPiece());
  };
  // Set up the magic .eden dir
  return root->getOrLoadChildTree(PathComponentPiece{kDotEdenName})
      .thenValue([=](TreeInodePtr dotEdenInode) {
        // We may be upgrading from an earlier build before we started
        // to use dot-eden symlinks, so we need to resolve or create
        // its inode here
        return dotEdenInode->getOrLoadChild(kDotEdenSymlinkName)
            .unit()
            .thenError(
                folly::tag_t<InodeError>{},
                [=](const InodeError& /*err*/) {
                  createDotEdenSymlink(dotEdenInode);
                })
            .ensure([=] {
              // Assign this number after we've fixed up the directory
              // contents, otherwise we'll lock ourselves out of
              // populating more entries.
              dotEdenInodeNumber_ = dotEdenInode->getNodeId();
            });
      })
      .thenError(
          folly::tag_t<facebook::eden::InodeError>{},
          [=](const InodeError& /*err*/) {
            auto dotEdenInode =
                getRootInode()->mkdir(PathComponentPiece{kDotEdenName}, 0744);
            dotEdenInode->symlink(
                "root"_pc, config_->getMountPath().stringPiece());
            dotEdenInode->symlink(
                "socket"_pc, serverState_->getSocketPath().stringPiece());
            dotEdenInode->symlink(
                "client"_pc, config_->getClientDirectory().stringPiece());

            createDotEdenSymlink(dotEdenInode);

            // We must assign this after we've built out the contents, otherwise
            // we'll lock ourselves out of populating more entries
            dotEdenInodeNumber_ = dotEdenInode->getNodeId();
          });
}

EdenMount::~EdenMount() {}

FOLLY_NODISCARD folly::Future<folly::Unit> EdenMount::addBindMount(
    RelativePathPiece repoPath,
    AbsolutePathPiece targetPath) {
  auto absRepoPath = getPath() + repoPath;

  // Sanity check that the mount point isn't pre-existing so that
  // we can show a nicer error message than we would otherwise.
  {
    auto bindMounts = bindMounts_.rlock();
    for (const auto& bindMount : *bindMounts) {
      if (bindMount.pathInMountDir == absRepoPath) {
        return folly::make_exception_wrapper<std::runtime_error>(
            folly::to<std::string>(
                "attempted to bind mount ",
                targetPath,
                " over ",
                bindMount.pathInMountDir,
                " but that path is already a bind mount for ",
                bindMount.pathInClientDir));
      }
    }
  }

  return this->ensureDirectoryExists(repoPath)
      .thenValue([this,
                  target = targetPath.copy(),
                  pathInMountDir = getPath() + repoPath](auto&&) {
        return serverState_->getPrivHelper()->bindMount(
            target.stringPiece(), pathInMountDir.stringPiece());
      })
      .thenValue([this,
                  target = targetPath.copy(),
                  pathInMountDir = getPath() + repoPath](auto&&) {
        // Record a successful mount into the list
        bindMounts_.wlock()->emplace_back(target, pathInMountDir);
      });
}

FOLLY_NODISCARD folly::Future<folly::Unit> EdenMount::removeBindMount(
    RelativePathPiece repoPath) {
  auto absRepoPath = getPath() + repoPath;
  return serverState_->getPrivHelper()
      ->bindUnMount(absRepoPath.stringPiece())
      .thenValue([this, absRepoPath](auto&&) {
        auto bindMounts = bindMounts_.wlock();
        bindMounts->erase(
            std::remove_if(
                bindMounts->begin(),
                bindMounts->end(),
                [&](const auto& bindMount) {
                  return bindMount.pathInMountDir == absRepoPath;
                }),
            bindMounts->end());
      });
}

Future<Unit> EdenMount::performBindMounts() {
  vector<Future<Unit>> futures;

  auto bindMounts = bindMounts_.rlock();
  for (const auto& bindMount : *bindMounts) {
    futures.push_back(folly::makeFutureWith([this, bindMount] {
      // Make sure that both pathInClientDir and pathInMountDir exist before we
      // attempt to perform the mount.
      boost::filesystem::path boostBindMountSrc{
          bindMount.pathInClientDir.value()};
      boost::filesystem::create_directories(boostBindMountSrc);

      // pathInMountDir is absolute, rather than relative from the mount point.
      // This unfortunately is hard to change because it's baked into the
      // takeover protocol. So relativize here.
      auto relativePath = getPath().relativize(bindMount.pathInMountDir);
      return this->ensureDirectoryExists(relativePath)
          .thenValue([this,
                      pathInClientDir = AbsolutePath{bindMount.pathInClientDir},
                      pathInMountDir =
                          AbsolutePath{bindMount.pathInMountDir}](auto&&) {
            return serverState_->getPrivHelper()->bindMount(
                pathInClientDir.stringPiece(), pathInMountDir.stringPiece());
          });
    }));
  }

  return folly::collectAll(futures).thenValue(
      [](std::vector<folly::Try<folly::Unit>> results) {
        std::vector<folly::exception_wrapper> errors;
        for (auto& result : results) {
          if (result.hasException()) {
            errors.push_back(result.exception());
          }
        }

        if (errors.empty()) {
          return folly::unit;
        } else {
          std::string message{"Error creating bind mounts:\n"};
          for (const auto& error : errors) {
            message += folly::to<std::string>("  ", error.what(), "\n");
          }
          throw EdenMountError{message};
        }
      });
}

bool EdenMount::tryToTransitionState(State expected, State newState) {
  return state_.compare_exchange_strong(
      expected, newState, std::memory_order_acq_rel);
}

void EdenMount::transitionState(State expected, State newState) {
  State found = expected;
  if (!state_.compare_exchange_strong(
          found, newState, std::memory_order_acq_rel)) {
    throw std::runtime_error(folly::to<std::string>(
        "unable to transition mount ",
        getPath(),
        " to state ",
        newState,
        ": expected to be in state ",
        expected,
        " but actually in ",
        found));
  }
}

void EdenMount::transitionToFuseInitializationErrorState() {
  auto oldState = State::STARTING;
  auto newState = State::FUSE_ERROR;
  if (!state_.compare_exchange_strong(
          oldState, newState, std::memory_order_acq_rel)) {
    switch (oldState) {
      case State::DESTROYING:
      case State::SHUTTING_DOWN:
      case State::SHUT_DOWN:
        break;

      case State::INIT_ERROR:
      case State::FUSE_ERROR:
      case State::INITIALIZED:
      case State::INITIALIZING:
      case State::RUNNING:
      case State::UNINITIALIZED:
        XLOG(ERR)
            << "FUSE initialization error occurred for an EdenMount in the unexpected "
            << oldState << " state";
        break;

      case State::STARTING:
        XLOG(FATAL)
            << "compare_exchange_strong failed when transitioning EdenMount's state from "
            << oldState << " to " << newState;
        break;
    }
  }
}

void EdenMount::destroy() {
  auto oldState = state_.exchange(State::DESTROYING, std::memory_order_acq_rel);
  switch (oldState) {
    case State::UNINITIALIZED:
    case State::INITIALIZING: {
      // The root inode may still be null here if we failed to load the root
      // inode.  In this case just delete ourselves immediately since we don't
      // have any inodes to unload.  shutdownImpl() requires the root inode be
      // loaded.
      if (!getRootInode()) {
        delete this;
      } else {
        // Call shutdownImpl() to destroy all loaded inodes.
        shutdownImpl(/*doTakeover=*/false);
      }
      return;
    }
    case State::INITIALIZED:
    case State::RUNNING:
    case State::STARTING:
    case State::INIT_ERROR:
    case State::FUSE_ERROR: {
      // Call shutdownImpl() to destroy all loaded inodes.
      shutdownImpl(/*doTakeover=*/false);
      return;
    }
    case State::SHUTTING_DOWN:
      // Nothing else to do.  shutdown() will destroy us when it completes.
      return;
    case State::SHUT_DOWN:
      // We were already shut down, and can delete ourselves immediately.
      XLOG(DBG1) << "destroying shut-down EdenMount " << getPath();
      delete this;
      return;
    case State::DESTROYING:
      // Fall through to the error handling code below.
      break;
  }

  XLOG(FATAL) << "EdenMount::destroy() called on mount " << getPath()
              << " in unexpected state " << oldState;
}

folly::SemiFuture<SerializedInodeMap> EdenMount::shutdown(
    bool doTakeover,
    bool allowFuseNotStarted) {
  // shutdown() should only be called on mounts that have not yet reached
  // SHUTTING_DOWN or later states.  Confirm this is the case, and move to
  // SHUTTING_DOWN.
  if (!(allowFuseNotStarted &&
        (tryToTransitionState(State::UNINITIALIZED, State::SHUTTING_DOWN) ||
         tryToTransitionState(State::INITIALIZING, State::SHUTTING_DOWN) ||
         tryToTransitionState(State::INITIALIZED, State::SHUTTING_DOWN))) &&
      !tryToTransitionState(State::RUNNING, State::SHUTTING_DOWN) &&
      !tryToTransitionState(State::STARTING, State::SHUTTING_DOWN) &&
      !tryToTransitionState(State::INIT_ERROR, State::SHUTTING_DOWN) &&
      !tryToTransitionState(State::FUSE_ERROR, State::SHUTTING_DOWN)) {
    EDEN_BUG() << "attempted to call shutdown() on a non-running EdenMount: "
               << "state was " << getState();
  }
  return shutdownImpl(doTakeover);
}

folly::SemiFuture<SerializedInodeMap> EdenMount::shutdownImpl(bool doTakeover) {
  journal_->cancelAllSubscribers();
  XLOG(DBG1) << "beginning shutdown for EdenMount " << getPath();

  return inodeMap_->shutdown(doTakeover)
      .thenValue([this](SerializedInodeMap inodeMap) {
        XLOG(DBG1) << "shutdown complete for EdenMount " << getPath();
        // Close the Overlay object to make sure we have released its lock.
        // This is important during graceful restart to ensure that we have
        // released the lock before the new edenfs process begins to take over
        // the mount point.
        overlay_->close();
        auto oldState =
            state_.exchange(State::SHUT_DOWN, std::memory_order_acq_rel);
        if (oldState == State::DESTROYING) {
          delete this;
        }
        return inodeMap;
      });
}

folly::Future<folly::Unit> EdenMount::unmount() {
  return folly::makeFutureWith([this] {
    auto mountingUnmountingState = mountingUnmountingState_.wlock();
    if (mountingUnmountingState->unmountStarted()) {
      return mountingUnmountingState->unmountPromise->getFuture();
    }
    mountingUnmountingState->unmountPromise.emplace();
    if (!mountingUnmountingState->fuseMountStarted()) {
      return folly::makeFuture();
    }
    auto mountFuture = mountingUnmountingState->fuseMountPromise->getFuture();
    mountingUnmountingState.unlock();

    return std::move(mountFuture)
        .thenTry([this](folly::Try<folly::Unit>&& mountResult) {
          if (mountResult.hasException()) {
            return folly::makeFuture();
          }
          return serverState_->getPrivHelper()->fuseUnmount(
              getPath().stringPiece());
        })
        .thenTry([this](
                     folly::Try<folly::Unit> &&
                     result) noexcept->folly::Future<Unit> {
          auto mountingUnmountingState = mountingUnmountingState_.wlock();
          DCHECK(mountingUnmountingState->unmountPromise.has_value());
          folly::SharedPromise<folly::Unit>* unsafeUnmountPromise =
              &*mountingUnmountingState->unmountPromise;
          mountingUnmountingState.unlock();

          unsafeUnmountPromise->setTry(folly::Try<Unit>{result});
          return folly::makeFuture<folly::Unit>(std::move(result));
        });
  });
}

const shared_ptr<UnboundedQueueExecutor>& EdenMount::getThreadPool() const {
  return serverState_->getThreadPool();
}

InodeMetadataTable* EdenMount::getInodeMetadataTable() const {
  return overlay_->getInodeMetadataTable();
}

FuseChannel* EdenMount::getFuseChannel() const {
  return channel_.get();
}

const AbsolutePath& EdenMount::getPath() const {
  return config_->getMountPath();
}

EdenStats* EdenMount::getStats() const {
  return &serverState_->getStats();
}

vector<BindMount> EdenMount::getBindMounts() const {
  return *bindMounts_.rlock();
}

TreeInodePtr EdenMount::getRootInode() const {
  return inodeMap_->getRootInode();
}

folly::Future<std::shared_ptr<const Tree>> EdenMount::getRootTreeFuture()
    const {
  auto commitHash = Hash{parentInfo_.rlock()->parents.parent1()};
  return objectStore_->getTreeForCommit(commitHash);
}

InodeNumber EdenMount::getDotEdenInodeNumber() const {
  return dotEdenInodeNumber_;
}

std::shared_ptr<const Tree> EdenMount::getRootTree() const {
  // TODO: We should convert callers of this API to use the Future-based
  // version.
  return getRootTreeFuture().get();
}

Future<InodePtr> EdenMount::getInode(RelativePathPiece path) const {
  return inodeMap_->getRootInode()->getChildRecursive(path);
}

folly::Future<InodePtr> EdenMount::resolveSymlink(InodePtr pInode) const {
  auto pathOptional = pInode->getPath();
  if (!pathOptional) {
    return makeFuture<InodePtr>(InodeError(ENOENT, pInode));
  }
  XLOG(DBG7) << "pathOptional.value() = " << pathOptional.value();
  return resolveSymlinkImpl(pInode, std::move(pathOptional.value()), 0);
}

folly::Future<InodePtr> EdenMount::resolveSymlinkImpl(
    InodePtr pInode,
    RelativePath&& path,
    size_t depth) const {
  if (++depth > kMaxSymlinkChainDepth) { // max chain length exceeded
    return makeFuture<InodePtr>(InodeError(ELOOP, pInode));
  }

  // if pInode is not a symlink => it's already "resolved", so just return it
  if (dtype_t::Symlink != pInode->getType()) {
    return makeFuture(pInode);
  }

  const auto fileInode = pInode.asFileOrNull();
  if (!fileInode) {
    auto bug = EDEN_BUG() << "all symlink inodes must be FileInodes: "
                          << pInode->getLogPath();
    return makeFuture<InodePtr>(bug.toException());
  }

  return fileInode->readlink(CacheHint::LikelyNeededAgain)
      .thenValue([this, pInode, path = std::move(path), depth](
                     std::string&& pointsTo) mutable {
        // normalized path to symlink target
        auto joinedExpected = joinAndNormalize(path.dirname(), pointsTo);
        if (joinedExpected.hasError()) {
          return makeFuture<InodePtr>(
              InodeError(joinedExpected.error(), pInode));
        }
        XLOG(DBG7) << "joinedExpected.value() = " << joinedExpected.value();
        // getting future below and doing .then on it are two separate
        // statements due to C++14 semantics (fixed in C++17) wherein RHS may be
        // executed before LHS, thus moving value of joinedExpected (in RHS)
        // before using it in LHS
        auto f =
            getInode(joinedExpected.value()); // get inode for symlink target
        return std::move(f).thenValue(
            [this, joinedPath = std::move(joinedExpected.value()), depth](
                InodePtr target) mutable {
              // follow the symlink chain recursively
              return resolveSymlinkImpl(target, std::move(joinedPath), depth);
            });
      });
}

folly::Future<std::vector<CheckoutConflict>> EdenMount::checkout(
    Hash snapshotHash,
    CheckoutMode checkoutMode) {
  // Hold the snapshot lock for the duration of the entire checkout operation.
  //
  // This prevents multiple checkout operations from running in parallel.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  auto ctx = std::make_shared<CheckoutContext>(
      this, std::move(parentsLock), checkoutMode);
  XLOG(DBG1) << "starting checkout for " << this->getPath() << ": "
             << oldParents << " to " << snapshotHash;

  // Update lastCheckoutTime_ before starting the checkout operation.
  // This ensures that any inode objects created once the checkout starts will
  // get the current checkout time, rather than the time from the previous
  // checkout
  *lastCheckoutTime_.wlock() = clock_->getRealtime();

  auto fromTreeFuture = objectStore_->getTreeForCommit(oldParents.parent1());
  auto toTreeFuture = objectStore_->getTreeForCommit(snapshotHash);

  auto journalDiffCallback = std::make_shared<JournalDiffCallback>();

  return folly::collect(fromTreeFuture, toTreeFuture)
      .thenValue([this, ctx, journalDiffCallback](
                     std::tuple<shared_ptr<const Tree>, shared_ptr<const Tree>>
                         treeResults) {
        auto& fromTree = std::get<0>(treeResults);
        auto& toTree = std::get<1>(treeResults);

        // Call JournalDiffCallback::performDiff() to compute the changes
        // between the original working directory state and the source tree
        // state.
        //
        // If we are doing a dry-run update we aren't going to create a journal
        // entry, so we can skip this step entirely.
        auto journalDiffFuture = Future<Unit>::makeEmpty();
        if (ctx->isDryRun()) {
          journalDiffFuture = makeFuture();
        } else {
          journalDiffFuture =
              journalDiffCallback->performDiff(this, getRootInode(), fromTree);
        }

        // Perform the requested checkout operation after the journal diff
        // completes.
        return std::move(journalDiffFuture)
            .thenValue([this, ctx, fromTree, toTree](auto&&) {
              ctx->start(this->acquireRenameLock());

              /**
               * If a significant number of tree inodes are loaded or referenced
               * by FUSE, then checkout is slow, because Eden must precisely
               * manage changes to each one, as if the checkout was actually
               * creating and removing files in each directory. If a tree is
               * unloaded and unmodified, Eden can pretend the checkout
               * operation blew away the entire subtree and assigned new inode
               * numbers to everything under it, which is much cheaper.
               *
               * To make checkout faster, enumerate all loaded, unreferenced
               * inodes and unload them, allowing checkout to use the fast path.
               *
               * Note that this will not unload any inodes currently referenced
               * by FUSE, including the kernel's cache, so rapidly switching
               * between commits while working should not be materially
               * affected.
               */
              this->getRootInode()->unloadChildrenUnreferencedByFuse();

              return this->getRootInode()->checkout(
                  ctx.get(), fromTree, toTree);
            });
      })
      .thenValue([ctx, snapshotHash](auto&&) {
        // Complete the checkout and save the new snapshot hash
        return ctx->finish(snapshotHash);
      })
      .thenValue([this, ctx, oldParents, snapshotHash, journalDiffCallback](
                     std::vector<CheckoutConflict>&& conflicts) {
        if (ctx->isDryRun()) {
          // This is a dry run, so all we need to do is tell the caller about
          // the conflicts: we should not modify any files or add any entries to
          // the journal.
          return std::move(conflicts);
        }

        // Save the new snapshot hash to the config
        // TODO: This should probably be done by CheckoutConflict::finish()
        // while still holding the parents lock.
        this->config_->setParentCommits(snapshotHash);
        XLOG(DBG1) << "updated snapshot for " << this->getPath() << " from "
                   << oldParents << " to " << snapshotHash;

        // Write a journal entry
        //
        // Note that we do not call journalDiffCallback->performDiff() a second
        // time here to compute the files that are now different from the
        // new state.  The checkout operation will only touch files that are
        // changed between fromTree and toTree.
        //
        // Any files that are unclean after the checkout operation must have
        // either been unclean before it started, or different between the
        // two trees.  Therefore the JournalDelta already includes information
        // that these files changed.
        auto uncleanPaths = journalDiffCallback->stealUncleanPaths();
        journal_->recordUncleanPaths(
            oldParents.parent1(), snapshotHash, std::move(uncleanPaths));

        return std::move(conflicts);
      });
}

folly::Future<folly::Unit> EdenMount::chown(uid_t uid, gid_t gid) {
  // 1) Ensure that all future opens will by default provide this owner
  setOwner(uid, gid);

  // 2) Modify all uids/gids of files stored in the overlay
  auto metadata = getInodeMetadataTable();
  XDCHECK(metadata) << "Unexpected null Metadata Table";
  metadata->forEachModify([&](auto& /* unusued */, auto& record) {
    record.uid = uid;
    record.gid = gid;
  });

  // Note that any files being created at this point are not
  // guaranteed to have the requested uid/gid, but that racyness is
  // consistent with the behavior of chown

  // 3) Invalidate all inodes that the kernel holds a reference to
  auto inodesToInvalidate = getInodeMap()->getReferencedInodes();
  auto fuseChannel = getFuseChannel();
  XDCHECK(fuseChannel) << "Unexpected null Fuse Channel";
  fuseChannel->invalidateInodes(folly::range(inodesToInvalidate));

  return fuseChannel->flushInvalidations();
}

std::unique_ptr<DiffContext> EdenMount::createDiffContext(
    InodeDiffCallback* callback,
    bool listIgnored) const {
  return make_unique<DiffContext>(
      callback,
      listIgnored,
      getObjectStore(),
      serverState_->getTopLevelIgnores());
}

Future<Unit> EdenMount::diff(const DiffContext* ctxPtr, Hash commitHash) const {
  auto rootInode = getRootInode();
  return objectStore_->getTreeForCommit(commitHash)
      .thenValue([ctxPtr, rootInode = std::move(rootInode)](
                     std::shared_ptr<const Tree>&& rootTree) {
        return rootInode->diff(
            ctxPtr,
            RelativePathPiece{},
            std::move(rootTree),
            ctxPtr->getToplevelIgnore(),
            false);
      });
}

Future<Unit> EdenMount::diff(
    InodeDiffCallback* callback,
    Hash commitHash,
    bool listIgnored) const {
  // Create a DiffContext object for this diff operation.
  auto context = createDiffContext(callback, listIgnored);
  const DiffContext* ctxPtr = context.get();

  // stateHolder() exists to ensure that the DiffContext and GitIgnoreStack
  // exists until the diff completes.
  auto stateHolder = [ctx = std::move(context)]() {};

  return diff(ctxPtr, commitHash).ensure(std::move(stateHolder));
}

void EdenMount::resetParents(const ParentCommits& parents) {
  // Hold the snapshot lock around the entire operation.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  XLOG(DBG1) << "resetting snapshot for " << this->getPath() << " from "
             << oldParents << " to " << parents;

  // TODO: Maybe we should walk the inodes and see if we can dematerialize some
  // files using the new source control state.

  config_->setParentCommits(parents);
  parentsLock->parents.setParents(parents);

  journal_->recordHashUpdate(oldParents.parent1(), parents.parent1());
}

struct timespec EdenMount::getLastCheckoutTime() const {
  return *lastCheckoutTime_.rlock();
}

void EdenMount::setLastCheckoutTime(
    std::chrono::system_clock::time_point time) {
  *lastCheckoutTime_.wlock() = folly::to<struct timespec>(time);
}

void EdenMount::resetParent(const Hash& parent) {
  resetParents(ParentCommits{parent});
}

RenameLock EdenMount::acquireRenameLock() {
  return RenameLock{this};
}

SharedRenameLock EdenMount::acquireSharedRenameLock() {
  return SharedRenameLock{this};
}

std::string EdenMount::getCounterName(CounterName name) {
  const auto& mountPath = getPath();
  const auto base = basename(mountPath.stringPiece());
  switch (name) {
    case CounterName::INODEMAP_LOADED:
      return folly::to<std::string>("inodemap.", base, ".loaded");
    case CounterName::INODEMAP_UNLOADED:
      return folly::to<std::string>("inodemap.", base, ".unloaded");
    case CounterName::JOURNAL_MEMORY:
      return folly::to<std::string>("journal.", base, ".memory");
    case CounterName::JOURNAL_ENTRIES:
      return folly::to<std::string>("journal.", base, ".count");
    case CounterName::JOURNAL_DURATION:
      return folly::to<std::string>("journal.", base, ".duration_secs");
    case CounterName::JOURNAL_MAX_FILES_ACCUMULATED:
      return folly::to<std::string>("journal.", base, ".files_accumulated.max");
  }
  EDEN_BUG() << "unknown counter name "
             << static_cast<std::underlying_type_t<CounterName>>(name);
  folly::assume_unreachable();
}

folly::Future<TakeoverData::MountInfo> EdenMount::getFuseCompletionFuture() {
  return fuseCompletionPromise_.getFuture();
}

folly::Future<folly::Unit> EdenMount::startFuse() {
  return folly::makeFutureWith([&]() {
    transitionState(
        /*expected=*/State::INITIALIZED, /*newState=*/State::STARTING);

    // Just in case the mount point directory doesn't exist,
    // automatically create it.
    boost::filesystem::path boostMountPath{getPath().value()};
    boost::filesystem::create_directories(boostMountPath);

    return fuseMount()
        .thenValue([this](folly::File&& fuseDevice) {
          createFuseChannel(std::move(fuseDevice));
          return channel_->initialize().thenValue(
              [this](FuseChannel::StopFuture&& fuseCompleteFuture) {
                fuseInitSuccessful(std::move(fuseCompleteFuture));
              });
        })
        .thenError([this](folly::exception_wrapper&& ew) {
          transitionToFuseInitializationErrorState();
          return makeFuture<folly::Unit>(std::move(ew));
        });
  });
}

void EdenMount::takeoverFuse(FuseChannelData takeoverData) {
  transitionState(State::INITIALIZED, State::STARTING);

  try {
    beginMount().setValue();

    createFuseChannel(std::move(takeoverData.fd));
    auto fuseCompleteFuture =
        channel_->initializeFromTakeover(takeoverData.connInfo);
    fuseInitSuccessful(std::move(fuseCompleteFuture));
  } catch (const std::exception& ex) {
    transitionToFuseInitializationErrorState();
    throw;
  }
}

folly::Future<folly::File> EdenMount::fuseMount() {
  return folly::makeFutureWith([&] { return &beginMount(); })
      .thenValue([this](folly::Promise<folly::Unit>* mountPromise) {
        AbsolutePath mountPath = getPath();
        return serverState_->getPrivHelper()
            ->fuseMount(mountPath.stringPiece())
            .thenTry(
                [mountPath, mountPromise, this](
                    folly::Try<folly::File>&& fuseDevice)
                    -> folly::Future<folly::File> {
                  if (fuseDevice.hasException()) {
                    mountPromise->setException(fuseDevice.exception());
                    return folly::makeFuture<folly::File>(
                        fuseDevice.exception());
                  }
                  if (mountingUnmountingState_.rlock()->unmountStarted()) {
                    fuseDevice->close();
                    return serverState_->getPrivHelper()
                        ->fuseUnmount(mountPath.stringPiece())
                        .thenError(
                            folly::tag<std::exception>,
                            [](std::exception&& unmountError) {
                              // TODO(strager): Should we make
                              // EdenMount::unmount() also fail with the same
                              // exception?
                              XLOG(ERR)
                                  << "fuseMount was cancelled, but rollback (fuseUnmount) failed: "
                                  << unmountError.what();
                              throw unmountError;
                            })
                        .thenValue([mountPath, mountPromise](folly::Unit&&) {
                          auto error = FuseDeviceUnmountedDuringInitialization{
                              mountPath};
                          mountPromise->setException(error);
                          return folly::makeFuture<folly::File>(error);
                        });
                  }

                  mountPromise->setValue();
                  return folly::makeFuture(std::move(fuseDevice).value());
                });
      });
}

folly::Promise<folly::Unit>& EdenMount::beginMount() {
  auto mountingUnmountingState = mountingUnmountingState_.wlock();
  if (mountingUnmountingState->fuseMountPromise.has_value()) {
    EDEN_BUG() << __func__ << " unexpectedly called more than once";
  }
  if (mountingUnmountingState->unmountStarted()) {
    throw EdenMountCancelled{};
  }
  mountingUnmountingState->fuseMountPromise.emplace();
  // N.B. Return a reference to the lock-protected fuseMountPromise member,
  // then release the lock. This is safe for two reasons:
  //
  // * *fuseMountPromise will never be destructed (e.g. by calling
  //   std::optional<>::reset()) or reassigned. (fuseMountPromise never goes
  //   from `has_value() == true` to `has_value() == false`.)
  //
  // * folly::Promise is self-synchronizing; getFuture() can be called
  //   concurrently with setValue()/setException().
  return *mountingUnmountingState->fuseMountPromise;
}

void EdenMount::createFuseChannel(folly::File fuseDevice) {
  channel_.reset(new FuseChannel(
      std::move(fuseDevice),
      getPath(),
      FLAGS_fuseNumThreads,
      dispatcher_.get(),
      serverState_->getProcessNameCache(),
      std::chrono::duration_cast<folly::Duration>(
          serverState_->getReloadableConfig()
              .getEdenConfig()
              ->getFuseRequestTimeout())));
}

void EdenMount::fuseInitSuccessful(
    FuseChannel::StopFuture&& fuseCompleteFuture) {
  // Try to transition to the RUNNING state.
  // This state transition could fail if shutdown() was called before we saw the
  // FUSE_INIT message from the kernel.
  transitionState(State::STARTING, State::RUNNING);

  std::move(fuseCompleteFuture)
      .via(serverState_->getThreadPool().get())
      .thenValue([this](FuseChannel::StopData&& stopData) {
        // If the FUSE device is no longer valid then the mount point has
        // been unmounted.
        if (!stopData.fuseDevice) {
          inodeMap_->setUnmounted();
        }

        std::vector<AbsolutePath> bindMounts;
        {
          auto locked = bindMounts_.rlock();
          for (const auto& entry : *locked) {
            bindMounts.push_back(entry.pathInMountDir);
          }
        }

        fuseCompletionPromise_.setValue(TakeoverData::MountInfo(
            getPath(),
            config_->getClientDirectory(),
            bindMounts,
            std::move(stopData.fuseDevice),
            stopData.fuseSettings,
            SerializedInodeMap{} // placeholder
            ));
      })
      .thenError([this](folly::exception_wrapper&& ew) {
        XLOG(ERR) << "session complete with err: " << ew.what();
        fuseCompletionPromise_.setException(std::move(ew));
      });
}

struct stat EdenMount::initStatData() const {
  struct stat st = {};

  auto owner = getOwner();
  st.st_uid = owner.uid;
  st.st_gid = owner.gid;
  // We don't really use the block size for anything.
  // 4096 is fairly standard for many file systems.
  st.st_blksize = 4096;

  return st;
}

InodeMetadata EdenMount::getInitialInodeMetadata(mode_t mode) const {
  auto owner = getOwner();
  return InodeMetadata{
      mode, owner.uid, owner.gid, InodeTimestamps{getLastCheckoutTime()}};
}

namespace {
Future<Unit> ensureDirectoryExistsHelper(
    TreeInodePtr parent,
    PathComponentPiece childName,
    RelativePathPiece rest) {
  auto contents = parent->getContents().rlock();
  if (auto* child = folly::get_ptr(contents->entries, childName)) {
    if (!child->isDirectory()) {
      throw InodeError(EEXIST, parent, childName);
    }

    contents.unlock();

    if (rest.empty()) {
      return folly::unit;
    }
    return parent->getOrLoadChildTree(childName).thenValue(
        [rest = RelativePath{rest}](TreeInodePtr child) {
          auto [nextChildName, nextRest] = splitFirst(rest);
          return ensureDirectoryExistsHelper(child, nextChildName, nextRest);
        });
  }

  contents.unlock();
  TreeInodePtr child;
  try {
    child = parent->mkdir(childName, S_IFDIR | 0755);
  } catch (std::system_error& e) {
    // If two threads are racing to create the subdirectory, that's fine, just
    // try again.
    if (e.code().value() == EEXIST) {
      return ensureDirectoryExistsHelper(parent, childName, rest);
    }
    throw;
  }
  if (rest.empty()) {
    return folly::unit;
  }
  auto [nextChildName, nextRest] = splitFirst(rest);
  return ensureDirectoryExistsHelper(child, nextChildName, nextRest);
}
} // namespace

Future<Unit> EdenMount::ensureDirectoryExists(RelativePathPiece fromRoot) {
  auto [childName, rest] = splitFirst(fromRoot);
  return ensureDirectoryExistsHelper(getRootInode(), childName, rest);
}

bool EdenMount::MountingUnmountingState::fuseMountStarted() const noexcept {
  return fuseMountPromise.has_value();
}

bool EdenMount::MountingUnmountingState::unmountStarted() const noexcept {
  return unmountPromise.has_value();
}

EdenMountCancelled::EdenMountCancelled()
    : std::runtime_error{"EdenMount was unmounted during initialization"} {}

} // namespace eden
} // namespace facebook
