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
#include "TreeEntryFileInode.h"
#include "TreeInodeDirHandle.h"
#include "eden/fs/overlay/Overlay.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/utils/PathFuncs.h"

namespace facebook {
namespace eden {

TreeInode::TreeInode(
    std::unique_ptr<Tree>&& tree,
    fusell::MountPoint* mountPoint,
    fuse_ino_t parent,
    fuse_ino_t ino,
    std::shared_ptr<LocalStore> store,
    std::shared_ptr<Overlay> overlay)
    : DirInode(ino),
      tree_(std::move(tree)),
      mount_(mountPoint),
      parent_(parent),
      ino_(ino),
      store_(std::move(store)),
      overlay_(std::move(overlay)) {}

TreeInode::TreeInode(
    fusell::MountPoint* mountPoint,
    fuse_ino_t parent,
    fuse_ino_t ino,
    std::shared_ptr<LocalStore> store,
    std::shared_ptr<Overlay> overlay)
    : DirInode(ino),
      mount_(mountPoint),
      parent_(parent),
      ino_(ino),
      store_(std::move(store)),
      overlay_(std::move(overlay)) {}

TreeInode::~TreeInode() {}

folly::Future<fusell::Dispatcher::Attr> TreeInode::getattr() {
  fusell::Dispatcher::Attr attr;

  attr.st.st_mode = S_IFDIR | 0755;
  attr.st.st_ino = ino_;

  return attr;
}

folly::Future<std::shared_ptr<fusell::InodeBase>> TreeInode::getChildByName(
    PathComponentPiece namepiece) {
  auto myname = fusell::InodeNameManager::get()->resolvePathToNode(ino_);
  auto overlay_contents = overlay_->readDir(myname);

  const auto& iter = overlay_contents.entries.find(namepiece.copy());
  if (iter != overlay_contents.entries.end()) {
    if (iter->second == dtype_t::Whiteout) {
      // This entry was deleted.
      folly::throwSystemErrorExplicit(ENOENT);
    }

    auto node = fusell::InodeNameManager::get()->getNodeByName(ino_, namepiece);

    if (iter->second == dtype_t::Dir) {
      return std::make_shared<TreeInode>(
          mount_, ino_, node->getNodeId(), store_, overlay_);
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
      auto node =
          fusell::InodeNameManager::get()->getNodeByName(ino_, namepiece);

      if (ent.getFileType() == FileType::DIRECTORY) {
        auto tree = store_->getTree(ent.getHash());
        return std::make_shared<TreeInode>(
            std::move(tree), mount_, ino_, node->getNodeId(), store_, overlay_);
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

folly::Future<fusell::DirHandle*> TreeInode::opendir(
    const struct fuse_file_info&) {
  return new TreeInodeDirHandle(
      std::static_pointer_cast<TreeInode>(shared_from_this()));
}

folly::Future<fusell::DirInode::CreateResult>
TreeInode::create(PathComponentPiece name, mode_t mode, int flags) {
  // Figure out the relative path to this inode.
  auto myname = fusell::InodeNameManager::get()->resolvePathToNode(ino_);

  // Compute the effective name of the node they want to create.
  auto targetname = myname + name;

  // Ask the overlay manager to create it.
  auto file = overlay_->openFile(targetname, O_CREAT | flags, mode);
  // Discard the file handle and allow the FileData class to open it again.
  // We'll need to figure out something nicer than this in a follow-on diff
  // to make sure that O_EXCL|O_CREAT is working correctly.
  file.close();

  // Generate an inode number for this new entry.
  auto node = fusell::InodeNameManager::get()->getNodeByName(ino_, name);

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
  return inode->open(fi).then([=](fusell::FileHandle* handle_ptr) {
    // Capture the handle into a unique_ptr so that we can ensure caleanup.
    // This will be removed when we remove the naked pointers in a followup.
    std::unique_ptr<fusell::FileHandle> handle(handle_ptr);

    // Now that we have the file handle, let's look up the attributes.
    return handle_ptr->getattr().then([ =, handle = std::move(handle) ](
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

std::shared_ptr<LocalStore> TreeInode::getStore() const {
  return store_;
}

std::shared_ptr<Overlay> TreeInode::getOverlay() const {
  return overlay_;
}

void TreeInode::performCheckout(
    const std::string& hash,
    fusell::InodeDispatcher* dispatcher,
    std::shared_ptr<fusell::MountPoint> mountPoint) {
  throw std::runtime_error("not yet implemented");
}
}
}
