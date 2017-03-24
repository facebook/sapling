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
#include <folly/futures/Future.h>
#include <glog/logging.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/CheckoutContext.h"
#include "eden/fs/inodes/DiffContext.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMounts.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeError.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/git/GitIgnoreStack.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

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

std::shared_ptr<EdenMount> EdenMount::makeShared(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore) {
  return std::shared_ptr<EdenMount>{
      new EdenMount{std::move(config), std::move(objectStore)},
      EdenMountDeleter{}};
}

EdenMount::EdenMount(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore)
    : config_(std::move(config)),
      inodeMap_{new InodeMap(this)},
      dispatcher_{new EdenDispatcher(this)},
      mountPoint_(
          new fusell::MountPoint(config_->getMountPath(), dispatcher_.get())),
      objectStore_(std::move(objectStore)),
      overlay_(std::make_shared<Overlay>(config_->getOverlayPath())),
      dirstate_(std::make_unique<Dirstate>(this)),
      bindMounts_(config_->getBindMounts()),
      mountGeneration_(globalProcessGeneration | ++mountGeneration) {
  // Load the overlay, if present.
  auto rootOverlayDir = overlay_->loadOverlayDir(FUSE_ROOT_ID);

  // Load the current snapshot ID from the on-disk config
  auto snapshotID = config_->getSnapshotID();
  *currentSnapshot_.wlock() = snapshotID;

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  TreeInodePtr rootInode;
  if (rootOverlayDir) {
    rootInode = TreeInodePtr::makeNew(this, std::move(rootOverlayDir.value()));
  } else {
    // Note: We immediately wait on the Future returned by
    // getTreeForCommit().
    //
    // Loading the root tree may take a while.  It may be better to refactor
    // the code slightly so that this is done in a helper function, before the
    // EdenMount constructor is called.
    auto rootTree = objectStore_->getTreeForCommit(snapshotID).get();
    rootInode = TreeInodePtr::makeNew(this, std::move(rootTree));
  }
  auto maxInodeNumber = overlay_->getMaxRecordedInode();
  inodeMap_->initialize(std::move(rootInode), maxInodeNumber);
  VLOG(2) << "Initializing eden mount " << getPath()
          << "; max existing inode number is " << maxInodeNumber;

  // Record the transition from no snapshot to the current snapshot in
  // the journal.  This also sets things up so that we can carry the
  // snapshot id forward through subsequent journal entries.
  auto delta = std::make_unique<JournalDelta>();
  delta->toHash = snapshotID;
  journal_.wlock()->addDelta(std::move(delta));
}

EdenMount::~EdenMount() {}

void EdenMount::destroy() {
  VLOG(1) << "beginning shutdown for EdenMount " << getPath();
  inodeMap_->beginShutdown();
}

void EdenMount::shutdownComplete() {
  VLOG(1) << "destruction complete for EdenMount " << getPath();
  delete this;
}

fusell::Channel* EdenMount::getFuseChannel() const {
  return mountPoint_->getChannel();
}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

TreeInodePtr EdenMount::getRootInode() const {
  return inodeMap_->getRootInode();
}

folly::Future<std::unique_ptr<Tree>> EdenMount::getRootTreeFuture() const {
  auto commitHash = Hash{*currentSnapshot_.rlock()};
  return objectStore_->getTreeForCommit(commitHash);
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
  auto snapshotLock = currentSnapshot_.wlock();
  auto oldSnapshot = *snapshotLock;
  auto ctx = std::make_shared<CheckoutContext>(std::move(snapshotLock), force);
  VLOG(1) << "starting checkout for " << this->getPath() << ": " << oldSnapshot
          << " to " << snapshotHash;

  auto fromTreeFuture = objectStore_->getTreeForCommit(oldSnapshot);
  auto toTreeFuture = objectStore_->getTreeForCommit(snapshotHash);

  return folly::collect(fromTreeFuture, toTreeFuture)
      .then([this, ctx](
          std::tuple<unique_ptr<Tree>, unique_ptr<Tree>> treeResults) {
        auto& fromTree = std::get<0>(treeResults);
        auto& toTree = std::get<1>(treeResults);
        ctx->start(this->acquireRenameLock());
        return this->getRootInode()->checkout(
            ctx.get(), std::move(fromTree), std::move(toTree));
      })
      .then([this, ctx, oldSnapshot, snapshotHash]() {
        // Save the new snapshot hash
        VLOG(1) << "updating snapshot for " << this->getPath() << " from "
                << oldSnapshot << " to " << snapshotHash;
        this->config_->setSnapshotID(snapshotHash);
        auto conflicts = ctx->finish(snapshotHash);

        // Write a journal entry
        // TODO: We don't include any file changes for now.  We'll need to
        // figure out the desired data to pass to watchman.  We intentionally
        // don't want to give it the full list of files that logically
        // changed--we intentionally don't process files that were changed but
        // have never been accessed.
        auto journalDelta = make_unique<JournalDelta>();
        journalDelta->fromHash = oldSnapshot;
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

  // TODO: Load the system-wide ignore settings and user-specific
  // ignore settings.
  auto ignore = make_unique<GitIgnoreStack>(nullptr);
  auto* ignorePtr = ignore.get();

  // stateHolder() exists to ensure that the DiffContext and GitIgnoreStack
  // exists until the diff completes.
  auto stateHolder =
      [ ctx = std::move(context), ignore = std::move(ignore) ](){};

  auto rootInode = getRootInode();
  return getRootTreeFuture()
      .then([ ctxPtr, ignorePtr, rootInode = std::move(rootInode) ](
          std::unique_ptr<Tree> && rootTree) {
        return rootInode->diff(
            ctxPtr, RelativePathPiece{}, std::move(rootTree), ignorePtr, false);
      })
      .ensure(std::move(stateHolder));
}

void EdenMount::resetCommit(Hash snapshotHash) {
  // We currently don't verify that snapshotHash refers to a valid commit
  // in the ObjectStore.  We could do that just for verification purposes.

  auto snapshotLock = currentSnapshot_.wlock();
  auto oldSnapshot = *snapshotLock;

  VLOG(1) << "resetting snapshot for " << this->getPath() << " from "
          << oldSnapshot << " to " << snapshotHash;
  *snapshotLock = snapshotHash;
  this->config_->setSnapshotID(snapshotHash);

  auto journalDelta = make_unique<JournalDelta>();
  journalDelta->fromHash = oldSnapshot;
  journalDelta->toHash = snapshotHash;
  journal_.wlock()->addDelta(std::move(journalDelta));
}

RenameLock EdenMount::acquireRenameLock() {
  return RenameLock{this};
}

SharedRenameLock EdenMount::acquireSharedRenameLock() {
  return SharedRenameLock{this};
}
}
} // facebook::eden
