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
#include <folly/File.h>
#include "TreeInode.h"
#include "eden/fs/model/Tree.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

class TreeEntryFileHandle;
class Blob;
class FileData;
class Hash;

class TreeEntryFileInode : public fusell::FileInode {
 public:
  /** Construct an inode using an optional entry (it may be nullptr) */
  TreeEntryFileInode(
      fuse_ino_t ino,
      std::shared_ptr<TreeInode> parentInode_,
      const TreeEntry* entry);

  /** Construct an inode using a freshly created overlay file.
   * file must be moved in and must have been created by a call to
   * Overlay::openFile.  This constructor is used in the DirInode::create
   * case and is required to implement O_EXCL correctly. */
  TreeEntryFileInode(
      fuse_ino_t ino,
      std::shared_ptr<TreeInode> parentInode,
      folly::File&& file);

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<fusell::Dispatcher::Attr> setattr(
      const struct stat& attr,
      int to_set) override;
  folly::Future<std::string> readlink() override;
  folly::Future<std::unique_ptr<fusell::FileHandle>> open(
      const struct fuse_file_info& fi) override;

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;
  folly::Future<Hash> getSHA1();

  const TreeEntry* getEntry() const;

  /// Ensure that underlying storage information is loaded
  std::shared_ptr<FileData> getOrLoadData();

 private:

  /// Called as part of shutting down an open handle.
  void fileHandleDidClose();

  /// Compute the path to the overlay file for this item.
  AbsolutePath getLocalPath() const;

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
