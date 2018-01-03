/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/EdenMount.h"

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <folly/ExceptionWrapper.h>
#include <folly/FBString.h>
#include <folly/File.h>
#include <folly/Subprocess.h>
#include <folly/chrono/Conv.h>
#include <folly/experimental/logging/Logger.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <folly/io/async/EventBase.h>
#include <folly/system/ThreadName.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeDiffCallback.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/Clock.h"

using folly::Future;
using folly::StringPiece;
using folly::Unit;
using folly::Unit;
using folly::makeFuture;
using folly::setThreadName;
using folly::to;
using std::chrono::system_clock;
using std::make_shared;
using std::make_unique;
using std::shared_ptr;
using std::unique_ptr;
using std::vector;

DEFINE_int32(fuseNumThreads, 16, "how many fuse dispatcher threads to spawn");

namespace facebook {
namespace eden {

namespace {
/**
 * Helper for computing unclean paths when changing parents
 *
 * This InodeDiffCallback instance is used to compute the set
 * of unclean files before and after actions that change the
 * current commit hash of the mount point.
 */
class JournalDiffCallback : public InodeDiffCallback {
 public:
  explicit JournalDiffCallback()
      : data_{folly::in_place, make_unique<JournalDelta>()} {}

  void ignoredFile(RelativePathPiece) override {}

  void untrackedFile(RelativePathPiece) override {}

  void removedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    data_.wlock()->journalDelta->uncleanPaths.insert(path.copy());
  }

  void modifiedFile(
      RelativePathPiece path,
      const TreeEntry& /* sourceControlEntry */) override {
    data_.wlock()->journalDelta->uncleanPaths.insert(path.copy());
  }

  void diffError(RelativePathPiece path, const folly::exception_wrapper& ew)
      override {
    // TODO: figure out what we should do to notify the user, if anything.
    // perhaps we should just add this path to the list of unclean files?
    XLOG(WARNING) << "error computing journal diff data for " << path << ": "
                  << folly::exceptionStr(ew);
  }

  FOLLY_NODISCARD Future<folly::Unit> performDiff(
      ObjectStore* objectStore,
      TreeInodePtr rootInode,
      std::shared_ptr<const Tree> rootTree) {
    auto diffContext =
        make_unique<DiffContext>(this, /* listIgnored = */ false, objectStore);

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

  /** moves the JournalDelta information out of this diff callback instance,
   * rendering it invalid */
  std::unique_ptr<JournalDelta> stealJournalDelta() {
    std::unique_ptr<JournalDelta> result;
    std::swap(result, data_.wlock()->journalDelta);

    return result;
  }

 private:
  struct Data {
    explicit Data(unique_ptr<JournalDelta>&& journalDelta)
        : journalDelta(std::move(journalDelta)) {}

    unique_ptr<JournalDelta> journalDelta;
  };
  folly::Synchronized<Data> data_;
};
} // namespace

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
    std::shared_ptr<Clock> clock) {
  auto mount = std::shared_ptr<EdenMount>{new EdenMount{std::move(config),
                                                        std::move(objectStore),
                                                        socketPath,
                                                        globalStats,
                                                        clock},
                                          EdenMountDeleter{}};
  return mount->initialize().then([mount] { return mount; });
}

EdenMount::EdenMount(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore,
    AbsolutePathPiece socketPath,
    fusell::ThreadLocalEdenStats* globalStats,
    std::shared_ptr<Clock> clock)
    : globalEdenStats_(globalStats),
      config_(std::move(config)),
      inodeMap_{new InodeMap(this)},
      dispatcher_{new EdenDispatcher(this)},
      objectStore_(std::move(objectStore)),
      overlay_(std::make_shared<Overlay>(config_->getOverlayPath())),
      bindMounts_(config_->getBindMounts()),
      mountGeneration_(globalProcessGeneration | ++mountGeneration),
      socketPath_(socketPath),
      straceLogger_{kEdenStracePrefix.str() +
                    config_->getMountPath().value().toStdString()},
      lastCheckoutTime_{clock->getRealtime()},
      path_(config_->getMountPath()),
      uid_(getuid()),
      gid_(getgid()),
      clock_(clock) {}

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
        journal_.addDelta(std::move(delta));
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
      .then([this](std::shared_ptr<const Tree> tree) {
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

void EdenMount::performBindMounts() {
  for (auto& bindMount : bindMounts_) {
    auto& pathInMountDir = bindMount.pathInMountDir;
    try {
      // If pathInMountDir does not exist, then it must be created before
      // the bind mount is performed.
      boost::system::error_code errorCode;
      boost::filesystem::path mountDir = pathInMountDir.c_str();
      boost::filesystem::create_directories(mountDir, errorCode);

      fusell::privilegedBindMount(
          bindMount.pathInClientDir.c_str(), pathInMountDir.c_str());
    } catch (...) {
      // Consider recording all failed bind mounts in a way that can be
      // communicated back to the caller in a structured way.
      XLOG(ERR) << "Failed to perform bind mount for "
                << pathInMountDir.stringPiece() << ".";
    }
  }
}

bool EdenMount::doStateTransition(State expected, State newState) {
  return state_.compare_exchange_strong(expected, newState);
}

void EdenMount::destroy() {
  auto oldState = state_.exchange(State::DESTROYING);
  switch (oldState) {
    case State::UNINITIALIZED: {
      // The root inode may still be null here if we failed to load the root
      // inode.  In this case just delete ourselves immediately since we don't
      // have any inodes to unload.  shutdownImpl() requires the root inode be
      // loaded.
      if (!getRootInode()) {
        delete this;
      } else {
        // Call shutdownImpl() to destroy all loaded inodes.
        shutdownImpl().then([this] { delete this; });
      }
      return;
    }
    case State::RUNNING:
    case State::STARTING:
    case State::FUSE_ERROR:
    case State::FUSE_DONE: {
      // Call shutdownImpl() to destroy all loaded inodes,
      // and delete ourselves when it completes.
      shutdownImpl().then([this] { delete this; });
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
              << " in unexpected state " << static_cast<uint32_t>(oldState);
}

Future<Unit> EdenMount::shutdown() {
  // shutdown() should only be called on mounts that have not yet reached
  // SHUTTING_DOWN or later states.  Confirm this is the case, and move to
  // SHUTTING_DOWN.
  if (!doStateTransition(State::RUNNING, State::SHUTTING_DOWN) &&
      !doStateTransition(State::STARTING, State::SHUTTING_DOWN) &&
      !doStateTransition(State::FUSE_DONE, State::SHUTTING_DOWN) &&
      !doStateTransition(State::FUSE_ERROR, State::SHUTTING_DOWN)) {
    EDEN_BUG() << "attempted to call shutdown() on a non-running EdenMount: "
               << "state was " << static_cast<uint32_t>(state_.load());
  }
  return shutdownImpl();
}

Future<Unit> EdenMount::shutdownImpl() {
  journal_.cancelAllSubscribers();
  XLOG(DBG1) << "beginning shutdown for EdenMount " << getPath();
  return inodeMap_->shutdown().then([this] {
    XLOG(DBG1) << "shutdown complete for EdenMount " << getPath();
    state_.store(State::SHUT_DOWN);
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

folly::Future<std::shared_ptr<const Tree>> EdenMount::getRootTreeFuture()
    const {
  auto commitHash = Hash{parentInfo_.rlock()->parents.parent1()};
  return objectStore_->getTreeForCommit(commitHash);
}

fusell::InodeNumber EdenMount::getDotEdenInodeNumber() const {
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
    CheckoutMode checkoutMode) {
  // Hold the snapshot lock for the duration of the entire checkout operation.
  //
  // This prevents multiple checkout operations from running in parallel.
  auto parentsLock = parentInfo_.wlock();
  auto oldParents = parentsLock->parents;
  auto ctx =
      std::make_shared<CheckoutContext>(std::move(parentsLock), checkoutMode);
  XLOG(DBG1) << "starting checkout for " << this->getPath() << ": "
             << oldParents << " to " << snapshotHash;

  // Update lastCheckoutTime_ before starting the checkout operation.
  // This ensures that any inode objects created once the checkout starts will
  // get the current checkout time, rather than the time from the previous
  // checkout
  *lastCheckoutTime_.wlock() = getCurrentCheckoutTime();

  auto fromTreeFuture = objectStore_->getTreeForCommit(oldParents.parent1());
  auto toTreeFuture = objectStore_->getTreeForCommit(snapshotHash);

  auto journalDiffCallback = std::make_shared<JournalDiffCallback>();

  return folly::collect(fromTreeFuture, toTreeFuture)
      .then([this, ctx, journalDiffCallback](
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
          journalDiffFuture = journalDiffCallback->performDiff(
              getObjectStore(), getRootInode(), fromTree);
        }

        // Perform the requested checkout operation after the journal diff
        // completes.
        return journalDiffFuture.then(
            [this, ctx, fromTree, toTree]() {
              ctx->start(this->acquireRenameLock());
              return this->getRootInode()
                  ->checkout(ctx.get(), fromTree, toTree)
                  .then([toTree]() mutable { return toTree; });
            });
      })
      .then([this, ctx, oldParents, snapshotHash, journalDiffCallback](
                std::shared_ptr<const Tree> toTree) {
        // Save the new snapshot hash
        auto conflicts = ctx->finish(snapshotHash);
        if (ctx->isDryRun()) {
          // This is a dry run, so all we need to do is tell the caller about
          // the conflicts: we should not modify any files or add any entries to
          // the journal.
          return conflicts;
        }

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
        auto journalDelta = journalDiffCallback->stealJournalDelta();
        journalDelta->fromHash = oldParents.parent1();
        journalDelta->toHash = snapshotHash;
        journal_.addDelta(std::move(journalDelta));

        return conflicts;
      });
}

Future<Unit> EdenMount::diff(const DiffContext* ctxPtr) const {
  auto rootInode = getRootInode();
  return getRootTreeFuture().then([ctxPtr, rootInode = std::move(rootInode)](
                                      std::shared_ptr<const Tree>&& rootTree) {
    return rootInode->diff(
        ctxPtr,
        RelativePathPiece{},
        std::move(rootTree),
        ctxPtr->getToplevelIgnore(),
        false);
  });
}

Future<Unit> EdenMount::diff(InodeDiffCallback* callback, bool listIgnored)
    const {
  // Create a DiffContext object for this diff operation.
  auto context =
      make_unique<DiffContext>(callback, listIgnored, getObjectStore());
  const DiffContext* ctxPtr = context.get();

  // stateHolder() exists to ensure that the DiffContext and GitIgnoreStack
  // exists until the diff completes.
  auto stateHolder = [ctx = std::move(context)]() {};

  return diff(ctxPtr).ensure(std::move(stateHolder));
}

folly::Future<folly::Unit> EdenMount::diffRevisions(
    InodeDiffCallback* callback,
    Hash fromHash,
    Hash toHash) {
  auto fromTreeFuture = objectStore_->getTreeForCommit(fromHash);
  auto toTreeFuture = objectStore_->getTreeForCommit(toHash);

  auto context = make_unique<DiffContext>(
      callback, /*listIgnored=*/false, getObjectStore());
  const DiffContext* ctxPtr = context.get();
  // stateHolder() exists to ensure that the DiffContext and GitIgnoreStack
  // exists until the diff completes.
  auto stateHolder = [ctx = std::move(context)]() {};

  return collectAll(fromTreeFuture, toTreeFuture)
      .then([this, ctxPtr](std::tuple<
                           folly::Try<std::shared_ptr<const Tree>>,
                           folly::Try<std::shared_ptr<const Tree>>>& tup) {
        auto fromTree = std::get<0>(tup).value();
        auto toTree = std::get<1>(tup).value();
        auto rootInode = TreeInodePtr::makeNew(this, std::move(fromTree));

        return rootInode->diff(
            ctxPtr,
            RelativePathPiece{},
            std::move(toTree),
            ctxPtr->getToplevelIgnore(),
            false);
      })
      .ensure(std::move(stateHolder));
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

  auto journalDelta = make_unique<JournalDelta>();
  journalDelta->fromHash = oldParents.parent1();
  journalDelta->toHash = parents.parent1();
  journal_.addDelta(std::move(journalDelta));
}

struct timespec EdenMount::getCurrentCheckoutTime() {
  return getClock().getRealtime();
}

struct timespec EdenMount::getLastCheckoutTime() {
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
  if (name == CounterName::LOADED) {
    return getPath().stringPiece().str() + ".loaded";
  } else if (name == CounterName::UNLOADED) {
    return getPath().stringPiece().str() + ".unloaded";
  }
  folly::throwSystemErrorExplicit(EINVAL, "unknown counter name", name);
}

folly::Future<folly::File> EdenMount::getFuseCompletionFuture() {
  return fuseCompletionPromise_.getFuture();
}

folly::Future<folly::Unit> EdenMount::startFuse(
    folly::EventBase* eventBase,
    std::shared_ptr<UnboundedQueueThreadPool> threadPool) {
  return folly::makeFutureWith([this, eventBase, threadPool] {
    if (!doStateTransition(State::UNINITIALIZED, State::STARTING)) {
      throw std::runtime_error("mount point has already been started");
    }

    eventBase_ = eventBase;
    threadPool_ = threadPool;

    auto fuseDevice = fusell::privilegedFuseMount(path_.stringPiece());
    channel_ = std::make_unique<fusell::FuseChannel>(
        std::move(fuseDevice),
        path_,
        eventBase_,
        FLAGS_fuseNumThreads,
        dispatcher_.get());

    // we'll use this shortly to wait until the mount is started successfully.
    auto initFuture = initFusePromise_.getFuture();

    channel_->getThreadsStoppingFuture().then([this] {
      // If threads are stopping before we got as far as setting the state to
      // RUNNING, we must have experienced an error
      if (doStateTransition(State::STARTING, State::FUSE_ERROR)) {
        initFusePromise_.setException(
            std::runtime_error("fuse session failed to initialize"));
      } else {
        // If we were RUNNING and a thread stopped, then record
        // that transition to FUSE_DONE.
        doStateTransition(State::RUNNING, State::FUSE_DONE);
      }
    });

    channel_->getSessionCompleteFuture().then([this] {
      // In case we are performing a graceful restart,
      // extract the fuse device now.
      folly::File fuseDevice = channel_->stealFuseDevice();
      channel_.reset();

      fuseCompletionPromise_.setValue(std::move(fuseDevice));
    });

    // wait for init to complete or error; this will throw an exception
    // if the init procedure failed.
    return initFuture;
  });
}

void EdenMount::mountStarted() {
  // Don't update status_ if it has already been put into an error
  // state or something.
  if (doStateTransition(State::STARTING, State::RUNNING)) {
    // Let ::start() know that we're up and running
    initFusePromise_.setValue();
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
} // namespace eden
} // namespace facebook
