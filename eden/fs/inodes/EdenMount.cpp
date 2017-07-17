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
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/MountPoint.h"
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

using std::make_unique;
using std::unique_ptr;
using std::vector;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;

namespace facebook {
namespace eden {

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
    folly::ThreadLocal<fusell::EdenStats>* globalStats) {
  auto mount = std::shared_ptr<EdenMount>{
      new EdenMount{
          std::move(config), std::move(objectStore), socketPath, globalStats},
      EdenMountDeleter{}};
  return mount->initialize().then([mount] { return mount; });
}

EdenMount::EdenMount(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore,
    AbsolutePathPiece socketPath,
    folly::ThreadLocal<fusell::EdenStats>* globalStats)
    : globalEdenStats_(globalStats),
      config_(std::move(config)),
      inodeMap_{new InodeMap(this)},
      dispatcher_{new EdenDispatcher(this)},
      mountPoint_(
          new fusell::MountPoint(config_->getMountPath(), dispatcher_.get())),
      objectStore_(std::move(objectStore)),
      overlay_(std::make_shared<Overlay>(config_->getOverlayPath())),
      dirstate_(std::make_unique<Dirstate>(this)),
      bindMounts_(config_->getBindMounts()),
      mountGeneration_(globalProcessGeneration | ++mountGeneration),
      socketPath_(socketPath) {}

folly::Future<folly::Unit> EdenMount::initialize() {
  auto parents = std::make_shared<ParentCommits>(config_->getParentCommits());
  parentCommits_.wlock()->setParents(*parents);

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
      .onError([=](const InodeError& err) {
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
  XLOG(DBG1) << "beginning shutdown for EdenMount " << getPath();
  inodeMap_->beginShutdown();
}

void EdenMount::shutdownComplete() {
  XLOG(DBG1) << "destruction complete for EdenMount " << getPath();
  delete this;
}

fusell::Channel* EdenMount::getFuseChannel() const {
  return mountPoint_->getChannel();
}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}

const AbsolutePath& EdenMount::getSocketPath() const {
  return socketPath_;
}

folly::ThreadLocal<fusell::EdenStats>* EdenMount::getStats() const {
  return globalEdenStats_;
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

TreeInodePtr EdenMount::getRootInode() const {
  return inodeMap_->getRootInode();
}

folly::Future<std::unique_ptr<Tree>> EdenMount::getRootTreeFuture() const {
  auto commitHash = Hash{parentCommits_.rlock()->parent1()};
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
  auto parentsLock = parentCommits_.wlock();
  auto oldParents = *parentsLock;
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
  auto parentsLock = parentCommits_.wlock();
  auto oldParents = *parentsLock;
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
        parentsLock->setParents(parents);

        auto journalDelta = make_unique<JournalDelta>();
        journalDelta->fromHash = oldParents.parent1();
        journalDelta->toHash = parents.parent1();
        journal_.wlock()->addDelta(std::move(journalDelta));
      });
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
}
} // facebook::eden
