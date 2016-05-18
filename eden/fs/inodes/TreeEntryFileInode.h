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
class FileData;

class TreeEntryFileInode : public fusell::FileInode {
 public:
  TreeEntryFileInode(
      fuse_ino_t ino,
      std::shared_ptr<TreeInode> parentInode_,
      const TreeEntry* entry);

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<std::string> readlink() override;
  folly::Future<std::unique_ptr<fusell::FileHandle>> open(
      const struct fuse_file_info& fi) override;

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;

  const TreeEntry* getEntry() const;

  /// Ensure that underlying storage information is loaded
  std::shared_ptr<FileData> getOrLoadData();

 private:

  /// Called as part of shutting down an open handle.
  void fileHandleDidClose();

  /// Compute the path to the overlay file for this item.
  AbsolutePath getLocalPath() const;

  fuse_ino_t ino_;
  // We hold the ref on the parentInode so that entry_ remains
  // valid while we're both alive
  std::shared_ptr<TreeInode> parentInode_;
  const TreeEntry* entry_;

  std::shared_ptr<FileData> data_;
  /// for managing consistency, especially when materializing.
  // The corresponding FileData instance tracked by data_ above
  // keeps a (non-owning) reference on this mutex and has methods
  // that will acquire this mutex.
  std::mutex mutex_;

  friend class TreeEntryFileHandle;
};
}
}
