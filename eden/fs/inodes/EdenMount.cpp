/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenMount.h"

#include <glog/logging.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMounts.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/InodeNameManager.h"
#include "eden/fuse/MountPoint.h"

using std::shared_ptr;
using std::unique_ptr;
using std::vector;

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

EdenMount::EdenMount(
    std::unique_ptr<ClientConfig> config,
    std::unique_ptr<ObjectStore> objectStore)
    : config_(std::move(config)),
      dispatcher_{new EdenDispatcher(this)},
      nameManager_{new fusell::InodeNameManager()},
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
  std::shared_ptr<TreeInode> rootInode;
  if (rootOverlayDir) {
    rootInode =
        std::make_shared<TreeInode>(this, std::move(rootOverlayDir.value()));
  } else {
    auto rootTree = objectStore_->getTreeForCommit(snapshotID);
    rootInode = std::make_shared<TreeInode>(this, std::move(rootTree));
  }
  dispatcher_->setRootInode(rootInode);

  // Record the transition from no snapshot to the current snapshot in
  // the journal.  This also sets things up so that we can carry the
  // snapshot id forward through subsequent journal entries.
  auto delta = std::make_unique<JournalDelta>();
  delta->toHash = snapshotID;
  journal_.wlock()->addDelta(std::move(delta));
}

EdenMount::~EdenMount() {}

const AbsolutePath& EdenMount::getPath() const {
  return mountPoint_->getPath();
}

const vector<BindMount>& EdenMount::getBindMounts() const {
  return bindMounts_;
}

std::shared_ptr<TreeInode> EdenMount::getRootInode() const {
  return dispatcher_->getRootInode();
}

std::unique_ptr<Tree> EdenMount::getRootTree() const {
  auto rootInode = getRootInode();
  {
    auto dir = rootInode->getContents().rlock();
    auto& rootTreeHash = dir->treeHash.value();
    auto tree = objectStore_->getTree(rootTreeHash);
    return tree;
  }
}

shared_ptr<InodeBase> EdenMount::getInodeBase(RelativePathPiece path) const {
  auto inodeBase = dispatcher_->getInode(FUSE_ROOT_ID);
  auto relativePath = RelativePathPiece{path};

  // Walk down to the path of interest.
  auto it = relativePath.paths().begin();
  while (it != relativePath.paths().end()) {
    // This will throw if there is no such entry.
    inodeBase =
        dispatcher_
            ->lookupInodeBase(inodeBase->getNodeId(), it.piece().basename())
            .get();
    ++it;
  }

  return inodeBase;
}

shared_ptr<TreeInode> EdenMount::getTreeInode(RelativePathPiece path) const {
  auto inodeBase = getInodeBase(path);
  auto treeInode = std::dynamic_pointer_cast<TreeInode>(inodeBase);
  if (treeInode) {
    return treeInode;
  } else {
    folly::throwSystemErrorExplicit(
        ENOTDIR, "not a directory: ", path.stringPiece());
  }
}

shared_ptr<FileInode> EdenMount::getFileInode(RelativePathPiece path) const {
  auto inodeBase = getInodeBase(path);
  auto fileInode = std::dynamic_pointer_cast<FileInode>(inodeBase);
  if (fileInode) {
    return fileInode;
  } else {
    folly::throwSystemErrorExplicit(
        EISDIR, "is a directory: ", path.stringPiece());
  }
}
}
} // facebook::eden
