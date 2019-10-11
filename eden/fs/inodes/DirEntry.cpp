/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"

/*
 * DirEntry relies on mode_t fitting in 30 bits. In practice, on every system
 * Eden is likely to run on, mode_t only uses around 17 bits.
 *
 * https://github.com/torvalds/linux/blob/master/include/uapi/linux/stat.h
 * https://opensource.apple.com/source/xnu/xnu-201.5/bsd/sys/stat.h.auto.html
 *
 * Statically assert that the top two bits aren't used by any standard
 * constants.
 */
static_assert(
    uint32_t{S_IFMT | S_IRWXU | S_IRWXG | S_IRWXO} <= 0x3FFFFFFFu,
    "standard constants shouldn't use top two bits");

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

InodeBase* DirEntry::clearInode() {
  DCHECK(hasInodePointer_);
  auto inode = inode_;
  hasInodePointer_ = false;
  inodeNumber_ = inode->getNodeId();
  return inode;
}

} // namespace eden
} // namespace facebook
