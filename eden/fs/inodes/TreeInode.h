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
#include "eden/fuse/Inodes.h"

namespace facebook {
namespace eden {

class EdenMount;
class Hash;
class ObjectStore;
class Overlay;
class Tree;
class TreeEntry;

// Represents a Tree instance in a form that FUSE can consume
class TreeInode : public fusell::DirInode {
 public:
  TreeInode(
      EdenMount* mount,
      std::unique_ptr<Tree>&& tree,
      fuse_ino_t parent,
      fuse_ino_t ino);

  /// Construct an inode that only has backing in the Overlay area
  TreeInode(EdenMount* mount, fuse_ino_t parent, fuse_ino_t ino);

  ~TreeInode();

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  folly::Future<std::shared_ptr<fusell::InodeBase>> getChildByName(
      PathComponentPiece namepiece) override;
  folly::Future<std::unique_ptr<fusell::DirHandle>> opendir(
      const struct fuse_file_info& fi) override;
  folly::Future<folly::Unit> rename(
      PathComponentPiece name,
      std::shared_ptr<DirInode> newParent,
      PathComponentPiece newName) override;

  const Tree* getTree() const;
  fuse_ino_t getParent() const;
  fuse_ino_t getInode() const;

  /**
   * Get the EdenMount that this TreeInode belongs to.
   *
   * The EdenMount is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.
   */
  EdenMount* getMount() const;

  /**
   * Get the ObjectStore for this mount point.
   *
   * The ObjectStore is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.  (The ObjectStore is owned by the EdenMount.)
   */
  ObjectStore* getStore() const;

  const std::shared_ptr<Overlay>& getOverlay() const;
  folly::Future<fusell::DirInode::CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags) override;

  folly::Future<fuse_entry_param> mkdir(PathComponentPiece name, mode_t mode)
      override;
  folly::Future<folly::Unit> unlink(PathComponentPiece name) override;
  folly::Future<folly::Unit> rmdir(PathComponentPiece name) override;

  /** Called in a thrift context to switch the active snapshot.
   * Since this is called in a thrift context, RequestData::get() won't
   * return the usual results and the appropriate information must
   * be passed down from the thrift server itself.
   */
  void performCheckout(const Hash& hash);

  fusell::InodeNameManager* getNameMgr() const;

 private:
  const TreeEntry* getTreeEntry(PathComponentPiece name);

  // The EdenMount object that this inode belongs to.
  // We store this as a raw pointer since the TreeInode is part of the mount
  // point.  The EdenMount should always exist longer than any inodes it
  // contains.  (Storing a shared_ptr to the EdenMount would introduce circular
  // references which are undesirable.)
  EdenMount* const mount_{nullptr};

  std::unique_ptr<Tree> tree_;
  fuse_ino_t parent_;
};
}
}
