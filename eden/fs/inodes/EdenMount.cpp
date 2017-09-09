/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenMount.h"

#include <folly/ExceptionWrapper.h>
#include <folly/FBString.h>
#include <folly/File.h>
#include <folly/ThreadName.h>
#include <folly/experimental/logging/Logger.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"

using std::make_unique;
using std::unique_ptr;
using std::vector;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
using folly::setThreadName;
using folly::to;

DEFINE_int32(fuseNumThreads, 16, "how many fuse dispatcher threads to spawn");

namespace facebook {
namespace eden {

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

folly::Future<std::shared_ptr<EdenMount>> EdenMount::create(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore,
    AbsolutePathPiece socketPath,
    fusell::ThreadLocalEdenStats* globalStats,
    std::chrono::system_clock::time_point lastCheckoutTime) {
  auto mount = std::shared_ptr<EdenMount>{new EdenMount{std::move(config),
                                                        std::move(objectStore),
                                                        socketPath,
                                                        globalStats,
                                                        lastCheckoutTime},
                                          EdenMountDeleter{}};
  return mount->initialize().then([mount] { return mount; });
}

EdenMount::EdenMount(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore,
    AbsolutePathPiece socketPath,
    fusell::ThreadLocalEdenStats* globalStats,
    std::chrono::system_clock::time_point lastCheckOutTime)
    : globalEdenStats_(globalStats),
      config_(std::move(config)),
      inodeMap_{new InodeMap(this)},
      dispatcher_{new EdenDispatcher(this)},
      objectStore_(std::move(objectStore)),
      overlay_(std::make_shared<Overlay>(config_->getOverlayPath())),
      dirstate_(std::make_unique<Dirstate>(this)),
      bindMounts_(config_->getBindMounts()),
      mountGeneration_(globalProcessGeneration | ++mountGeneration),
      socketPath_(socketPath),
      straceLogger_{kEdenStracePrefix.str() +
                    config_->getMountPath().value().toStdString()},
      lastCheckoutTime_(lastCheckOutTime),
      path_(config_->getMountPath()),
      uid_(getuid()),
      gid_(getgid()) {}

folly::Future<folly::Unit> EdenMount::initialize() {
  auto parents = std::make_shared<ParentCommits>(config_->getParentCommits());
  parentInfo_.wlock()->parents.setParents(*parents);

  return createRootInode(*parents).then(
      [this, parents](TreeInodePtr initTreeNode) {
        auto maxInodeNumber = overlay_->getMaxRecordedInode();
        inodeMap_->initialize(std::move(initTreeNode), maxInodeNumber);
        XLOG(DBG2) << "Initializing eden mount " << getPath()
                   << "; max existing inode number is " << maxInodeNumber;

        // Record the transition from no snapshot to the current snapshot in
        // the journal.  This also sets things up so that we can carry the
        // snapshot id forward through subsequent journal entries.
        auto delta = std::make_unique<JournalDelta>();
        delta->toHash = parents->parent1();
        journal_.wlock()->addDelta(std::move(delta));
        return setupDotEden(getRootInode());
      });
}

folly::Future<TreeInodePtr> EdenMount::createRootInode(
    const ParentCommits& parentCommits) {
  // Load the overlay, if present.
  auto rootOverlayDir = overlay_->loadOverlayDir(FUSE_ROOT_ID);
  if (rootOverlayDir) {
    return folly::makeFuture<TreeInodePtr>(
        TreeInodePtr::makeNew(this, std::move(rootOverlayDir.value())));
  }
  return objectStore_->getTreeForCommit(parentCommits.parent1())
      .then([this](std::unique_ptr<Tree> tree) {
        return TreeInodePtr::makeNew(this, std::move(tree));
      });
}

folly::Future<folly::Unit> EdenMount::setupDotEden(TreeInodePtr root) {
  // Set up the magic .eden dir
  return root->getOrLoadChildTree(PathComponentPiece{kDotEdenName})
      .then([=](TreeInodePtr dotEdenInode) {
        // We could perhaps do something here to ensure that it reflects the
        // current state of the world, but for the moment we trust that it
        // still reflects how things were when we set it up.
        dotEdenInodeNumber_ = dotEdenInode->getNodeId();
      })
      .onError([=](const InodeError& /*err*/) {
        auto dotEdenInode =
            getRootInode()->mkdir(PathComponentPiece{kDotEdenName}, 0744);
        dotEdenInodeNumber_ = dotEdenInode->getNodeId();
        dotEdenInode->symlink(
            PathComponentPiece{"root"}, config_->getMountPath().stringPiece());
        dotEdenInode->symlink(
            PathComponentPiece{"socket"}, getSocketPath().stringPiece());
        dotEdenInode->symlink(
            PathComponentPiece{"client"},
            config_->getClientDirectory().stringPiece());
      });
}

EdenMount::~EdenMount() {}

void EdenMount::destroy() {
  auto oldState = state_.exchange(State::DESTROYING);
  if (oldState == State::RUNNING) {
    // Start the shutdown ourselves.
    // Use shutdownImpl() since we have already updated state_ to DESTROYING.
    auto shutdownFuture = shutdownImpl();
    // We intentionally ignore the returned future.
    // shutdown() will automatically destroy us when it completes now that
    // we have set the state to DESTROYING
    (void)shutdownFuture;
    return;
  } else if (oldState == State::SHUTTING_DOWN) {
    // Nothing else to do.  shutdown() will destroy us when it completes.
    return;
  } else if (oldState == State::SHUT_DOWN) {
    // We were already shut down, and can delete ourselves immediately.
    XLOG(DBG1) << "destroying shut-down EdenMount " << getPath();
    delete this;
  } else {
    // No other states should be possible.
    XLOG(FATAL) << "EdenMount::destroy() called on mount " << getPath()
                << " in unexpected state " << static_cast<uint32_t>(oldState);
  }
}

Future<Unit> EdenMount::shutdown() {
  // shutdown() should only be called on mounts in the RUNNING state.
  // Confirm this is the case, and move to SHUTTING_DOWN.
  auto expected = State::RUNNING;
  if (!state_.compare_exchange_strong(expected, State::SHUTTING_DOWN)) {
    EDEN_BUG() << "attempted to call shutdown() on a non-running EdenMount: "
               << "state was " << static_cast<uint32_t>(expected);
  }
  return shutdownImpl();
}

Future<Unit> EdenMount::shutdownImpl() {
  XLOG(DBG1) << "beginning shutdown for EdenMount " << getPath();
  return inodeMap_->shutdown().then([this] {
    auto oldState = state_.exchange(State::SHUT_DOWN);
    if (oldState == State::DESTROYING) {
      XLOG(DBG1) << "destroying EdenMount " << getPath()
                 << " after shutdown completion";
      delete this;
      return;
    }
    XLOG(DBG1) << "shutdown complete for EdenMount " << getPath();
  });
}

fusell::FuseChannel* EdenMount::getFuseChannel() const {
  return channel_.get();
}

const AbsolutePath& EdenMount::getPath() const {
  return path_;
}

const AbsolutePath& EdenMount::getSocketPath() const {
  return socketPath_;
}

fusell::ThreadLocalEdenStats* EdenMount::getStats() const {
  return globalEdenStats_;
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

TreeInodePtr EdenMount::getRootInode() const {
  return inodeMap_->getRootInode();
}

folly::Future<std::unique_ptr<Tree>> EdenMount::getRootTreeFuture() const {
  auto commitHash = Hash{parentInfo_.rlock()->parents.parent1()};
  return objectStore_->getTreeForCommit(commitHash);
}

fuse_ino_t EdenMount::getDotEdenInodeNumber() const {
  return dotEdenInodeNumber_;
}

std::unique_ptr<Tree> EdenMount::getRootTree() const {
  // TODO: We should convert callers of this API to use the Future-based
  // version.
  return getRootTreeFuture().get();
}

Future<InodePtr> EdenMount::getInode(RelativePathPiece path) const {
  return inodeMap_->getRootInode()->getChildRecursive(path);
}

InodePtr EdenMount::getInodeBlocking(RelativePathPiece path) const {
  return getInode(path).get();
}

TreeInodePtr EdenMount::getTreeInodeBlocking(RelativePathPiece path) const {
  return getInodeBlocking(path).asTreePtr();
}

FileInodePtr EdenMount::getFileInodeBlocking(RelativePathPiece path) const {
  return getInodeBlocking(path).asFilePtr();
}

folly::Future<std::vector<CheckoutConflict>> EdenMount::checkout(
    Hash snapshotHash,
    bool force) {
  // Hold the snapshot lock for the duration of the entire checkout operation.
  //
  // This prevents multiple checkout operations from running in parallel.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  auto ctx = std::make_shared<CheckoutContext>(std::move(parentsLock), force);
  XLOG(DBG1) << "starting checkout for " << this->getPath() << ": "
             << oldParents << " to " << snapshotHash;

  auto fromTreeFuture = objectStore_->getTreeForCommit(oldParents.parent1());
  auto toTreeFuture = objectStore_->getTreeForCommit(snapshotHash);

  return folly::collect(fromTreeFuture, toTreeFuture)
      .then([this,
             ctx](std::tuple<unique_ptr<Tree>, unique_ptr<Tree>> treeResults) {
        auto& fromTree = std::get<0>(treeResults);
        auto& toTree = std::get<1>(treeResults);

        // TODO: We should change the code to use shared_ptr<Tree>.
        // The ObjectStore should always return shared_ptrs so it can cache
        // them if we want to do so in the future.
        auto toTreeCopy = make_unique<Tree>(*toTree);

        ctx->start(this->acquireRenameLock());
        return this->getRootInode()
            ->checkout(ctx.get(), std::move(fromTree), std::move(toTree))
            .then([toTreeCopy = std::move(toTreeCopy)]() mutable {
              return std::move(toTreeCopy);
            });
      })
      .then([this](std::unique_ptr<Tree> toTree) {
        return dirstate_->onSnapshotChanged(toTree.get());
      })
      .then([this, ctx, oldParents, snapshotHash]() {
        // Save the new snapshot hash
        XLOG(DBG1) << "updating snapshot for " << this->getPath() << " from "
                   << oldParents << " to " << snapshotHash;
        this->config_->setParentCommits(snapshotHash);
        auto conflicts = ctx->finish(snapshotHash);

        // Write a journal entry
        // TODO: We don't include any file changes for now.  We'll need to
        // figure out the desired data to pass to watchman.  We intentionally
        // don't want to give it the full list of files that logically
        // changed--we intentionally don't process files that were changed but
        // have never been accessed.
        auto journalDelta = make_unique<JournalDelta>();
        journalDelta->fromHash = oldParents.parent1();
        journalDelta->toHash = snapshotHash;
        journal_.wlock()->addDelta(std::move(journalDelta));

        return conflicts;
      });
}

Future<Unit> EdenMount::diff(InodeDiffCallback* callback, bool listIgnored) {
  // Create a DiffContext object for this diff operation.
  auto context =
      make_unique<DiffContext>(callback, listIgnored, getObjectStore());
  const DiffContext* ctxPtr = context.get();

  // stateHolder() exists to ensure that the DiffContext and GitIgnoreStack
  // exists until the diff completes.
  auto stateHolder = [ctx = std::move(context)](){};

  auto rootInode = getRootInode();
  return getRootTreeFuture()
      .then([ ctxPtr, rootInode = std::move(rootInode) ](
          std::unique_ptr<Tree> && rootTree) {
        return rootInode->diff(
            ctxPtr,
            RelativePathPiece{},
            std::move(rootTree),
            ctxPtr->getToplevelIgnore(),
            false);
      })
      .ensure(std::move(stateHolder));
}

Future<Unit> EdenMount::resetParents(const ParentCommits& parents) {
  // Hold the snapshot lock around the entire operation.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  XLOG(DBG1) << "resetting snapshot for " << this->getPath() << " from "
             << oldParents << " to " << parents;

  // TODO: Maybe we should walk the inodes and see if we can dematerialize some
  // files using the new source control state.
  //
  // It probably makes sense to do this if/when we convert the Dirstate user
  // directives into a tree-like data structure.

  return objectStore_->getTreeForCommit(parents.parent1())
      .then([this](std::unique_ptr<Tree> rootTree) {
        return dirstate_->onSnapshotChanged(rootTree.get());
      })
      .then([
        this,
        parents,
        oldParents,
        parentsLock = std::move(parentsLock)
      ]() {
        this->config_->setParentCommits(parents);
        parentsLock->parents.setParents(parents);

        auto journalDelta = make_unique<JournalDelta>();
        journalDelta->fromHash = oldParents.parent1();
        journalDelta->toHash = parents.parent1();
        journal_.wlock()->addDelta(std::move(journalDelta));
      });
}

struct timespec EdenMount::getLastCheckoutTime() {
  auto lastCheckoutTime = lastCheckoutTime_;
  auto epochTime = lastCheckoutTime.time_since_epoch();
  auto epochSeconds =
      std::chrono::duration_cast<std::chrono::seconds>(epochTime);
  auto nsec = std::chrono::duration_cast<std::chrono::nanoseconds>(
      epochTime - epochSeconds);

  struct timespec time;
  time.tv_sec = epochSeconds.count();
  time.tv_nsec = nsec.count();
  return time;
}

Future<Unit> EdenMount::resetParent(const Hash& parent) {
  return resetParents(ParentCommits{parent});
}

RenameLock EdenMount::acquireRenameLock() {
  return RenameLock{this};
}

SharedRenameLock EdenMount::acquireSharedRenameLock() {
  return SharedRenameLock{this};
}

std::string EdenMount::getCounterName(CounterName name) {
  if (name == CounterName::LOADED) {
    return getPath().stringPiece().str() + ".loaded";
  } else if (name == CounterName::UNLOADED) {
    return getPath().stringPiece().str() + ".unloaded";
  }
  folly::throwSystemErrorExplicit(EINVAL, "unknown counter name", name);
}

void EdenMount::start(
    folly::EventBase* eventBase,
    std::function<void()> onStop,
    bool debug) {
  std::unique_lock<std::mutex> lock(mutex_);
  if (fuseStatus_ != FuseStatus::UNINIT) {
    throw std::runtime_error("mount point has already been started");
  }

  eventBase_ = eventBase;
  onStop_ = onStop;

  fuseStatus_ = FuseStatus::STARTING;

  auto fuseDevice = fusell::privilegedFuseMount(path_.stringPiece());
  channel_ = std::make_unique<fusell::FuseChannel>(
      std::move(fuseDevice), debug, dispatcher_.get());

  // Now, while holding the initialization mutex, start up the workers.
  threads_.reserve(FLAGS_fuseNumThreads);
  for (auto i = 0; i < FLAGS_fuseNumThreads; ++i) {
    threads_.emplace_back(std::thread([this] { fuseWorkerThread(); }));
  }

  // Wait until the mount is started successfully.
  while (fuseStatus_ == FuseStatus::STARTING) {
    statusCV_.wait(lock);
  }
  if (fuseStatus_ == FuseStatus::ERROR) {
    throw std::runtime_error("fuse session failed to initialize");
  }
}

void EdenMount::mountStarted() {
  std::lock_guard<std::mutex> guard(mutex_);
  // Don't update status_ if it has already been put into an error
  // state or something.
  if (fuseStatus_ == FuseStatus::STARTING) {
    fuseStatus_ = FuseStatus::RUNNING;
    statusCV_.notify_one();
  }
}

void EdenMount::fuseWorkerThread() {
  setThreadName(to<std::string>("fuse", path_.basename()));

  // The channel is responsible for running the loop.  It will
  // continue to do so until the fuse session is exited, either
  // due to error or because the filesystem was unmounted, or
  // because FuseChannel::requestSessionExit() was called.
  channel_->processSession();

  bool shouldCallonStop = false;
  bool shouldJoin = false;

  {
    std::lock_guard<std::mutex> guard(mutex_);
    if (fuseStatus_ == FuseStatus::STARTING) {
      // If we didn't get as far as setting the state to RUNNING,
      // we must have experienced an error
      fuseStatus_ = FuseStatus::ERROR;
      statusCV_.notify_one();
      shouldJoin = true;
    } else if (fuseStatus_ == FuseStatus::RUNNING) {
      // We are the first one to stop, so we get to share the news.
      fuseStatus_ = FuseStatus::STOPPING;
      shouldCallonStop = true;
      shouldJoin = true;
    }
  }

  if (shouldJoin) {
    // We are the first thread to exit the loop; we get to arrange
    // to join and notify the server of our completion
    eventBase_->runInEventBaseThread([this, shouldCallonStop] {
      // Wait for all workers to be done
      for (auto& thr : threads_) {
        thr.join();
      }

      // and tear down the fuse session.  For a graceful restart,
      // we will want to FuseChannel::stealFuseDevice() before
      // this point, or perhaps pass it through the onStop_
      // call.
      channel_.reset();

      // Do a little dance to steal ownership of the indirect
      // reference to the EdenMount that is held by the
      // onStop_ function; we can't leave it owned by ourselves
      // because that reference will block the completion of
      // the shutdown future.
      std::function<void()> stopper;
      std::swap(stopper, onStop_);

      // And let the edenMount know that all is done
      if (shouldCallonStop) {
        stopper();
      }
    });
  }
}

struct stat EdenMount::initStatData() const {
  struct stat st;
  memset(&st, 0, sizeof(st));

  st.st_uid = uid_;
  st.st_gid = gid_;
  // We don't really use the block size for anything.
  // 4096 is fairly standard for many file systems.
  st.st_blksize = 4096;

  return st;
}
}
} // facebook::eden
