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
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeEntry.h"
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
    : DirInode(ino), mount_(mount), tree_(std::move(tree)), parent_(parent) {}

TreeInode::TreeInode(EdenMount* mount, fuse_ino_t parent, fuse_ino_t ino)
    : DirInode(ino), mount_(mount), parent_(parent) {}

TreeInode::~TreeInode() {}

folly::Future<fusell::Dispatcher::Attr> TreeInode::getattr() {
  fusell::Dispatcher::Attr attr;

  attr.st.st_mode = S_IFDIR | 0755;
  attr.st.st_ino = getNodeId();
  // TODO: set nlink.  It should be 2 plus the number of subdirectories
  // TODO: set atime, mtime, and ctime

  return attr;
}

folly::Future<std::shared_ptr<fusell::InodeBase>> TreeInode::getChildByName(
    PathComponentPiece namepiece) {
  auto myname = getNameMgr()->resolvePathToNode(getNodeId());
  auto overlay_contents = getOverlay()->readDir(myname);

  const auto& iter = overlay_contents.entries.find(namepiece.copy());
  if (iter != overlay_contents.entries.end()) {
    if (iter->second == dtype_t::Whiteout) {
      // This entry was deleted.
      folly::throwSystemErrorExplicit(ENOENT);
    }

    auto node = getNameMgr()->getNodeByName(getNodeId(), namepiece);

    if (iter->second == dtype_t::Dir) {
      if (!overlay_contents.isOpaque) {
        // Check to see if we also have a TreeEntry for this directory
        const auto* entry = getTreeEntry(namepiece);
        if (entry != nullptr && entry->getFileType() == FileType::DIRECTORY) {
          auto tree = getStore()->getTree(entry->getHash());
          return std::make_shared<TreeInode>(
              mount_, std::move(tree), getNodeId(), node->getNodeId());
        }
      }
      // No corresponding TreeEntry, this exists only in the overlay.
      return std::make_shared<TreeInode>(
          mount_, getNodeId(), node->getNodeId());
    }

    return std::make_shared<TreeEntryFileInode>(
        node->getNodeId(),
        std::static_pointer_cast<TreeInode>(shared_from_this()),
        nullptr);
  }

  if (overlay_contents.isOpaque) {
    // No tree, or nothing from the tree is visible.
    folly::throwSystemErrorExplicit(ENOENT);
  }

  const auto* ent = getTreeEntry(namepiece);
  if (ent != nullptr) {
    auto node = getNameMgr()->getNodeByName(getNodeId(), namepiece);

    if (ent->getFileType() == FileType::DIRECTORY) {
      auto tree = getStore()->getTree(ent->getHash());
      return std::make_shared<TreeInode>(
          mount_, std::move(tree), getNodeId(), node->getNodeId());
    }

    return std::make_shared<TreeEntryFileInode>(
        node->getNodeId(),
        std::static_pointer_cast<TreeInode>(shared_from_this()),
        ent);
  }

  // No matching entry with that name
  folly::throwSystemErrorExplicit(ENOENT);
}

const TreeEntry* TreeInode::getTreeEntry(PathComponentPiece name) {
  if (!tree_) {
    return nullptr;
  }

  return tree_->getEntryPtr(name);
}

const Tree* TreeInode::getTree() const {
  return tree_.get();
}

fuse_ino_t TreeInode::getParent() const {
  return parent_;
}

fuse_ino_t TreeInode::getInode() const {
  return getNodeId();
}

folly::Future<std::unique_ptr<fusell::DirHandle>> TreeInode::opendir(
    const struct fuse_file_info&) {
  return std::make_unique<TreeInodeDirHandle>(
      std::static_pointer_cast<TreeInode>(shared_from_this()));
}

folly::Future<fusell::DirInode::CreateResult>
TreeInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Figure out the relative path to this inode.
  auto myname = getNameMgr()->resolvePathToNode(getNodeId());

  // Compute the effective name of the node they want to create.
  auto targetname = myname + name;

  // Ask the overlay manager to create it.
  // Since we will move this file into the underlying file data, we
  // take special care to ensure that it is opened read-write
  auto file = getOverlay()->openFile(
      targetname, O_RDWR | O_CREAT | (flags & ~(O_RDONLY | O_WRONLY)), 0600);

  // Generate an inode number for this new entry.
  auto node = getNameMgr()->getNodeByName(getNodeId(), name);

  // build a corresponding TreeEntryFileInode
  auto inode = std::make_shared<TreeEntryFileInode>(
      node->getNodeId(),
      std::static_pointer_cast<TreeInode>(shared_from_this()),
      std::move(file));

  fuse_file_info fi;
  memset(&fi, 0, sizeof(fi));

  // The kernel wants an open operation to return the inode,
  // the file handle and some attribute information.
  // Let's open a file handle now.
  return inode->open(fi).then([=](std::unique_ptr<fusell::FileHandle> handle) {
    // Now that we have the file handle, let's look up the attributes.
    auto getattrResult = handle->getattr();
    return getattrResult.then([ =, handle = std::move(handle) ](
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
  auto myname = getNameMgr()->resolvePathToNode(getNodeId());

  // Compute the effective name of the node they want to create.
  auto targetName = myname + name;

  // Will throw if we can't make the dir.
  getOverlay()->makeDir(targetName, mode);

  // Look up the inode for this new dir and return its entry info.
  return getMount()->getMountPoint()->getDispatcher()->lookup(
      getNodeId(), name);
}

folly::Future<folly::Unit> TreeInode::unlink(PathComponentPiece name) {
  // This will throw ENOENT if the name doesn't exist.
  auto inode = getChildByName(name).get();

  auto treeInode = std::dynamic_pointer_cast<TreeInode>(inode);
  if (treeInode) {
    folly::throwSystemErrorExplicit(EISDIR, "cannot unlink a dir");
  }
  // Compute the full name of the node they want to remove.
  auto myname = getNameMgr()->resolvePathToNode(getNodeId());
  auto targetName = myname + name;

  auto needWhiteout = getTreeEntry(name) != nullptr;
  auto overlay = getOverlay();
  overlay->removeFile(targetName, needWhiteout);
  return folly::Unit{};
}

folly::Future<folly::Unit> TreeInode::rmdir(PathComponentPiece name) {
  // This will throw ENOENT if the name doesn't exist.
  auto inode = getChildByName(name).get();

  auto treeInode = std::dynamic_pointer_cast<TreeInode>(inode);
  if (!treeInode) {
    folly::throwSystemErrorExplicit(ENOTDIR, "rmdir used on a file");
  }

  // Compute the full name of the node they want to remove.
  auto myname = getNameMgr()->resolvePathToNode(getNodeId());
  auto targetName = myname + name;

  auto childEntry = getTreeEntry(name);
  auto needWhiteout = childEntry != nullptr;
  auto overlay = getOverlay();

  // Pre-condition for removing a dir is that it must be empty;
  auto overlay_contents = overlay->readDir(targetName);

  if (!overlay_contents.isOpaque && childEntry) {
    auto childTree = getStore()->getTree(childEntry->getHash());
    // Check for any tree entries that are not marked as removed in the overlay
    for (auto& treeEntry : childTree->getTreeEntries()) {
      auto overlayIter = overlay_contents.entries.find(treeEntry.getName());
      if (overlayIter != overlay_contents.entries.end()) {
        if (overlayIter->second == dtype_t::Whiteout) {
          // This entry is marked as deleted, so it doesn't count here
          continue;
        }
      }
      folly::throwSystemErrorExplicit(
          ENOTEMPTY, "rmdir used on dir that is not empty (children in Tree)");
    }
  }

  for (auto& ent : overlay_contents.entries) {
    if (ent.second != dtype_t::Whiteout) {
      folly::throwSystemErrorExplicit(
          ENOTEMPTY,
          "rmdir used on dir that is not empty (children in Overlay)");
    }
  }

  overlay->removeDir(targetName, needWhiteout);
  return folly::Unit{};
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
