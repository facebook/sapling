/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "TreeInode.h"

#include "EdenMount.h"
#include "TreeEntryFileInode.h"
#include "TreeInodeDirHandle.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fuse/MountPoint.h"
#include "eden/fuse/RequestData.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

TreeInode::TreeInode(
    EdenMount* mount,
    std::unique_ptr<Tree>&& tree,
    fuse_ino_t parent,
    fuse_ino_t ino)
    : DirInode(ino),
      mount_(mount),
      tree_(std::move(tree)),
      parent_(parent),
      ino_(ino) {}

TreeInode::TreeInode(EdenMount* mount, fuse_ino_t parent, fuse_ino_t ino)
    : DirInode(ino), mount_(mount), parent_(parent), ino_(ino) {}

TreeInode::~TreeInode() {}

folly::Future<fusell::Dispatcher::Attr> TreeInode::getattr() {
  fusell::Dispatcher::Attr attr;

  attr.st.st_mode = S_IFDIR | 0755;
  attr.st.st_ino = ino_;

  return attr;
}

folly::Future<std::shared_ptr<fusell::InodeBase>> TreeInode::getChildByName(
    PathComponentPiece namepiece) {
  auto myname = getNameMgr()->resolvePathToNode(ino_);
  auto overlay_contents = getOverlay()->readDir(myname);

  const auto& iter = overlay_contents.entries.find(namepiece.copy());
  if (iter != overlay_contents.entries.end()) {
    if (iter->second == dtype_t::Whiteout) {
      // This entry was deleted.
      folly::throwSystemErrorExplicit(ENOENT);
    }

    auto node = getNameMgr()->getNodeByName(ino_, namepiece);

    if (iter->second == dtype_t::Dir) {
      return std::make_shared<TreeInode>(mount_, ino_, node->getNodeId());
    }

    return std::make_shared<TreeEntryFileInode>(
        node->getNodeId(),
        std::static_pointer_cast<TreeInode>(shared_from_this()),
        nullptr);
  }

  if (!tree_ || overlay_contents.isOpaque) {
    // No tree, or nothing from the tree is visible.
    folly::throwSystemErrorExplicit(ENOENT);
  }

  for (const auto& ent : tree_->getTreeEntries()) {
    if (ent.getName() == namepiece.stringPiece()) {
      auto node = getNameMgr()->getNodeByName(ino_, namepiece);

      if (ent.getFileType() == FileType::DIRECTORY) {
        auto tree = getStore()->getTree(ent.getHash());
        return std::make_shared<TreeInode>(
            mount_, std::move(tree), ino_, node->getNodeId());
      }

      return std::make_shared<TreeEntryFileInode>(
          node->getNodeId(),
          std::static_pointer_cast<TreeInode>(shared_from_this()),
          &ent);
    }
  }

  // No matching entry with that name
  folly::throwSystemErrorExplicit(ENOENT);
}

const Tree* TreeInode::getTree() const {
  return tree_.get();
}

fuse_ino_t TreeInode::getParent() const {
  return parent_;
}

fuse_ino_t TreeInode::getInode() const {
  return ino_;
}

folly::Future<std::unique_ptr<fusell::DirHandle>> TreeInode::opendir(
    const struct fuse_file_info&) {
  return std::make_unique<TreeInodeDirHandle>(
      std::static_pointer_cast<TreeInode>(shared_from_this()));
}

folly::Future<fusell::DirInode::CreateResult>
TreeInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Figure out the relative path to this inode.
  auto myname = getNameMgr()->resolvePathToNode(ino_);

  // Compute the effective name of the node they want to create.
  auto targetname = myname + name;

  // Ask the overlay manager to create it.
  auto file = getOverlay()->openFile(targetname, O_CREAT | flags, mode);
  // Discard the file handle and allow the FileData class to open it again.
  // We'll need to figure out something nicer than this in a follow-on diff
  // to make sure that O_EXCL|O_CREAT is working correctly.
  file.close();

  // Generate an inode number for this new entry.
  auto node = getNameMgr()->getNodeByName(ino_, name);

  // build a corresponding TreeEntryFileInode
  auto inode = std::make_shared<TreeEntryFileInode>(
      node->getNodeId(),
      std::static_pointer_cast<TreeInode>(shared_from_this()),
      nullptr);

  fuse_file_info fi;
  memset(&fi, 0, sizeof(fi));

  // The kernel wants an open operation to return the inode,
  // the file handle and some attribute information.
  // Let's open a file handle now.
  return inode->open(fi).then([=](std::unique_ptr<fusell::FileHandle> handle) {
    // Now that we have the file handle, let's look up the attributes.
    return handle->getattr().then([ =, handle = std::move(handle) ](
        fusell::Dispatcher::Attr attr) mutable {
      fusell::DirInode::CreateResult result;

      // Return all of the results back to the kernel.
      result.inode = inode;
      result.file = std::move(handle);
      result.attr = attr;
      result.node = node;

      return result;
    });
  });
}

folly::Future<fuse_entry_param> TreeInode::mkdir(
    PathComponentPiece name,
    mode_t mode) {
  // Figure out the relative path to this inode.
  auto myname = getNameMgr()->resolvePathToNode(ino_);

  // Compute the effective name of the node they want to create.
  auto targetName = myname + name;

  // Will throw if we can't make the dir.
  getOverlay()->makeDir(targetName, mode);

  // Look up the inode for this new dir and return its entry info.
  return getMount()->getMountPoint()->getDispatcher()->lookup(
      getNodeId(), name);
}

EdenMount* TreeInode::getMount() const {
  return mount_;
}

fusell::InodeNameManager* TreeInode::getNameMgr() const {
  return mount_->getMountPoint()->getNameMgr();
}

ObjectStore* TreeInode::getStore() const {
  return mount_->getObjectStore();
}

const std::shared_ptr<Overlay>& TreeInode::getOverlay() const {
  return mount_->getOverlay();
}

void TreeInode::performCheckout(const Hash& hash) {
  throw std::runtime_error("not yet implemented");
}
}
}
