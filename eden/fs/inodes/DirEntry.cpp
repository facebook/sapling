/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"

namespace facebook {
namespace eden {

InodeNumber DirEntry::getInodeNumber() const {
  return hasInodePointer_ ? inode_->getNodeId() : inodeNumber_;
}

FileInodePtr DirEntry::asFilePtrOrNull() const {
  if (hasInodePointer_) {
    if (auto file = dynamic_cast<FileInode*>(inode_)) {
      return FileInodePtr::newPtrLocked(file);
    }
  }
  return FileInodePtr{};
}

TreeInodePtr DirEntry::asTreePtrOrNull() const {
  if (hasInodePointer_) {
    if (auto tree = dynamic_cast<TreeInode*>(inode_)) {
      return TreeInodePtr::newPtrLocked(tree);
    }
  }
  return TreeInodePtr{};
}

void DirEntry::setInode(InodeBase* inode) {
  DCHECK(!hasInodePointer_);
  DCHECK(inode);
  DCHECK_EQ(inodeNumber_, inode->getNodeId());
  hasInodePointer_ = true;
  inode_ = inode;
}

void DirEntry::clearInode() {
  DCHECK(hasInodePointer_);
  hasInodePointer_ = false;
  auto inode = inode_;
  inodeNumber_ = inode->getNodeId();
}

} // namespace eden
} // namespace facebook
