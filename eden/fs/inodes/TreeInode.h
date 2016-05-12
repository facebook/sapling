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
#include "eden/fs/model/Tree.h"
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

class LocalStore;
class Overlay;

namespace fusell {
class MountPoint;
}

// Represents a Tree instance in a form that FUSE can consume
class TreeInode : public fusell::DirInode {
 public:
  TreeInode(
      std::unique_ptr<Tree>&& tree,
      fusell::MountPoint* mountPoint,
      fuse_ino_t parent,
      fuse_ino_t ino,
      std::shared_ptr<LocalStore> store,
      std::shared_ptr<Overlay> overlay);

  /// Construct an inode that only has backing in the Overlay area
  TreeInode(
      fusell::MountPoint* mountPoint,
      fuse_ino_t parent,
      fuse_ino_t ino,
      std::shared_ptr<LocalStore> store,
      std::shared_ptr<Overlay> overlay);

  ~TreeInode();

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<std::shared_ptr<fusell::InodeBase>> getChildByName(
      PathComponentPiece namepiece) override;
  folly::Future<fusell::DirHandle*> opendir(
      const struct fuse_file_info& fi) override;

  const Tree* getTree() const;
  fuse_ino_t getParent() const;
  fuse_ino_t getInode() const;
  std::shared_ptr<LocalStore> getStore() const;
  std::shared_ptr<Overlay> getOverlay() const;
  folly::Future<fusell::DirInode::CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags) override;

  /** Called in a thrift context to switch the active snapshot.
   * Since this is called in a thrift context, RequestData::get() won't
   * return the usual results and the appropriate information must
   * be passed down from the thrift server itself.
   */
  void performCheckout(
      const std::string& hash,
      fusell::InodeDispatcher* dispatcher,
      std::shared_ptr<fusell::MountPoint> mountPoint);

 private:
  std::unique_ptr<Tree> tree_;
  fusell::MountPoint* const mount_{nullptr};
  fuse_ino_t parent_;
  fuse_ino_t ino_;
  std::shared_ptr<LocalStore> store_;
  std::shared_ptr<Overlay> overlay_;
};
}
}
