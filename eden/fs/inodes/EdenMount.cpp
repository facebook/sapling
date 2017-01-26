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
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"

using std::unique_ptr;
using std::vector;
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;

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
  auto rootOverlayDir = overlay_->loadOverlayDir(RelativePathPiece());

  // Create the inode for the root of the tree using the hash contained
  // within the snapshotPath file
  auto snapshotID = config_->getSnapshotID();
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
  inodeMap_->setRootInode(std::move(rootInode));

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

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

TreeInodePtr EdenMount::getRootInode() const {
  return inodeMap_->getRootInode();
}

std::unique_ptr<Tree> EdenMount::getRootTree() const {
  auto rootInode = inodeMap_->getRootInode();
  {
    auto dir = rootInode->getContents().rlock();
    auto& rootTreeHash = dir->treeHash.value();
    auto tree = objectStore_->getTree(rootTreeHash);
    return tree;
  }
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

RenameLock EdenMount::acquireRenameLock() {
  return RenameLock{this};
}

SharedRenameLock EdenMount::acquireSharedRenameLock() {
  return SharedRenameLock{this};
}
}
} // facebook::eden
