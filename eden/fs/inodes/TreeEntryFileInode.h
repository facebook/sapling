/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include "TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

class TreeEntryFileHandle;
class Blob;

class TreeEntryFileInode : public fusell::FileInode {
 public:
  TreeEntryFileInode(
      fuse_ino_t ino,
      std::shared_ptr<TreeInode> parentInode_,
      const TreeEntry* entry);

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<std::string> readlink() override;
  folly::Future<fusell::FileHandle*> open(
      const struct fuse_file_info& fi) override;

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;

  const TreeEntry* getEntry() const;

 private:
  /// Ensure that blob_ is loaded (if appropriate) and bump openCount_
  void prepOpenState();

  /// Called as part of shutting down an open handle.
  void fileHandleDidClose();

  fuse_ino_t ino_;
  // We hold the ref on the parentInode so that entry_ remains
  // valid while we're both alive
  std::shared_ptr<TreeInode> parentInode_;
  const TreeEntry* entry_;
  /// how many open handles reference this inode
  size_t openCount_{0};
  /// if backed by tree, the data from the tree, else nullptr.
  // only valid if openCount_ > 0, else nullptr.
  std::unique_ptr<Blob> blob_;
  /// for managing consistency, especially when materializing.
  std::mutex mutex_;

  friend class TreeEntryFileHandle;
};
}
}
