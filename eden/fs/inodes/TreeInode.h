/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Optional.h>
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Hash.h"
#include "eden/utils/PathMap.h"

namespace facebook {
namespace eden {

class CheckoutAction;
class CheckoutContext;
class EdenMount;
class FileHandle;
class InodeMap;
class ObjectStore;
class Overlay;
class RenameLock;
class Tree;
class TreeEntry;

/**
 * Represents a directory in the file system.
 */
class TreeInode : public InodeBase {
 public:
  /**
   * Represents a directory entry.
   *
   * A directory entry can be in one of several states:
   *
   * - An InodeBase object for the entry may or may not exist.  If it does
   *   exist, it is the authoritative source of data for the entry.
   *
   * - If the child InodeBase object does not exist, we may or may not have an
   *   inode number already allocated for the child.  An inode number can be
   *   allocated on-demand if necessary, without fully creating a child
   *   InodeBase object.
   *
   * - The child may or may not be materialized in the overlay.
   *
   *   If the child contents are identical to an existing source control Tree
   *   or Blob then it does not need to be materialized, and the Entry may only
   *   contain the hash identifying the Tree/Blob.
   *
   *   If the child is materialized in the overlay, then it must have an inode
   *   number allocated to it.
   */
  struct Entry {
   public:
    /**
     * Create a hash for a non-materialized entry.
     */
    Entry(mode_t m, Hash hash) : mode(m), hash_{hash} {}

    /**
     * Create a hash for a materialized entry.
     */
    Entry(mode_t m, fuse_ino_t number) : mode(m), inodeNumber_{number} {}

    Entry(Entry&& e) = default;
    Entry& operator=(Entry&& e) = default;

    bool isMaterialized() const {
      // TODO: In the future we should probably only allow callers to invoke
      // this method when inode is not set.  If inode is set it should be the
      // authoritative source of data.
      return !hash_.hasValue();
    }
    Hash getHash() const {
      // TODO: In the future we should probably only allow callers to invoke
      // this method when inode is not set.  If inode is set it should be the
      // authoritative source of data.
      return hash_.value();
    }

    bool hasInodeNumber() const {
      return inodeNumber_ != 0;
    }
    fuse_ino_t getInodeNumber() const {
      DCHECK_NE(inodeNumber_, 0);
      return inodeNumber_;
    }
    void setInodeNumber(fuse_ino_t number) {
      DCHECK_EQ(inodeNumber_, 0);
      DCHECK(!inode);
      inodeNumber_ = number;
    }

    void setMaterialized(fuse_ino_t inode) {
      DCHECK(inodeNumber_ == 0 || inode == inodeNumber_);
      inodeNumber_ = inode;
      hash_.clear();
    }
    void setUnmaterialized(Hash hash) {
      hash_ = hash;
    }

    // TODO: Make mode private and provide an accessor method instead
    /** The complete st_mode value for this entry */
    mode_t mode{0};

   private:
    /**
     * If the entry is not materialized, this contains the hash
     * identifying the source control Tree (if this is a directory) or Blob
     * (if this is a file) that contains the entry contents.
     *
     * If the entry is materialized, this field is not set.
     *
     * TODO: If inode is set, this field generally should not be used, and the
     * child InodeBase should be consulted instead.
     */
    folly::Optional<Hash> hash_;

    /**
     * The inode number, if one is allocated for this entry, or 0 if one is not
     * allocated.
     *
     * An inode number is required for materialized entries, so this is always
     * non-zero if hash_ is not set.  (It may also be non-zero even when hash_
     * is set.)
     */
    fuse_ino_t inodeNumber_{0};

   public:
    // TODO: Make inode private and provide an accessor method instead
    /**
     * A pointer to the child inode, if it is loaded, or null if it is not
     * loaded.
     *
     * Note that we store this as a raw pointer.  Children inodes hold a
     * reference to their parent TreeInode, not the other way around.
     * Children inodes can be destroyed only in one of two ways:
     * - Being unlinked, then having their last reference go away.
     *   In this case they will be removed from our entries list when they are
     *   unlinked.
     * - Being unloaded (after their reference count is already 0).  In this
     *   case the parent TreeInodes responsible for triggering unloading of its
     *   children, so it resets this pointer to null when it unloads the child.
     */
    InodeBase* inode{nullptr};
  };

  /** Represents a directory in the overlay */
  struct Dir {
    /** The direct children of this directory */
    PathMap<std::unique_ptr<Entry>> entries;
    /** If the origin of this dir was a Tree, the hash of that tree */
    folly::Optional<Hash> treeHash;

    /** true if the dir has been materialized to the overlay.
     * If the contents match the original tree, this is false. */
    bool materialized{false};
  };

  /** Holds the results of a create operation.
   *
   * It is important that the file handle creation respect O_EXCL if
   * it set in the flags parameter to TreeInode::create.
   */
  struct CreateResult {
    /// file attributes and cache ttls.
    fusell::Dispatcher::Attr attr;
    /// The newly created inode instance.
    InodePtr inode;
    /// The newly opened file handle.
    std::shared_ptr<FileHandle> file;

    explicit CreateResult(const fusell::MountPoint* mount) : attr(mount) {}
  };

  TreeInode(
      fuse_ino_t ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      Entry* entry,
      std::unique_ptr<Tree>&& tree);

  /// Construct an inode that only has backing in the Overlay area
  TreeInode(
      fuse_ino_t ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      Entry* entry,
      Dir&& dir);

  /// Constructors for the root TreeInode
  TreeInode(EdenMount* mount, std::unique_ptr<Tree>&& tree);
  TreeInode(EdenMount* mount, Dir&& tree);

  ~TreeInode();

  folly::Future<fusell::Dispatcher::Attr> getattr() override;
  fusell::Dispatcher::Attr getAttrLocked(const Dir* contents);

  /** Implements the InodeBase method used by the Dispatcher
   * to create the Inode instance for a given name */
  folly::Future<InodePtr> getChildByName(PathComponentPiece namepiece);

  /**
   * Get the inode object for a child of this directory.
   *
   * The Inode object will be loaded if it is not already loaded.
   */
  folly::Future<InodePtr> getOrLoadChild(PathComponentPiece name);
  folly::Future<TreeInodePtr> getOrLoadChildTree(PathComponentPiece name);

  /**
   * Recursively look up a child inode.
   *
   * The Inode object in question, and all intervening TreeInode objects,
   * will be loaded if they are not already loaded.
   */
  folly::Future<InodePtr> getChildRecursive(RelativePathPiece name);

  fuse_ino_t getChildInodeNumber(PathComponentPiece name);

  folly::Future<std::shared_ptr<fusell::DirHandle>> opendir(
      const struct fuse_file_info& fi);
  folly::Future<folly::Unit> rename(
      PathComponentPiece name,
      TreeInodePtr newParent,
      PathComponentPiece newName);

  const folly::Synchronized<Dir>& getContents() const {
    return contents_;
  }
  folly::Synchronized<Dir>& getContents() {
    return contents_;
  }

  /**
   * Get the InodeMap for this tree's EdenMount.
   *
   * The InodeMap is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.
   */
  InodeMap* getInodeMap() const;

  /**
   * Get the ObjectStore for this mount point.
   *
   * The ObjectStore is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.  (The ObjectStore is owned by the EdenMount.)
   */
  ObjectStore* getStore() const;

  const std::shared_ptr<Overlay>& getOverlay() const;
  folly::Future<CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags);
  FileInodePtr symlink(PathComponentPiece name, folly::StringPiece contents);

  TreeInodePtr mkdir(PathComponentPiece name, mode_t mode);
  folly::Future<folly::Unit> unlink(PathComponentPiece name);
  folly::Future<folly::Unit> rmdir(PathComponentPiece name);

  /**
   * Update this directory so that it matches the specified source control Tree
   * object.
   *
   * @param ctx The CheckoutContext for the current checkout operation.
   *     The caller guarantees that the CheckoutContext argument will remain
   *     valid until the returned Future completes.
   * @param fromTree The Tree object that the checkout operation is moving
   *     from.  This argument is necessary to detect conflicts between the
   *     current inode state and the expected previous source control state.
   *     This argument may be null when updating a TreeInode that did not exist
   *     in source control in the previous commit state.
   * @param toTree The Tree object that the checkout operation is moving to.
   *     This argument must not be null.  (In order to remove a file or tree
   *     that does not exist in the new commit, checkoutRemoveChild() must be
   *     invoked on the parent TreeInode instead.)
   *
   * @return Returns a future that will be fulfilled once this tree and all of
   *     its children have been updated.
   */
  folly::Future<folly::Unit> checkout(
      CheckoutContext* ctx,
      std::unique_ptr<Tree> fromTree,
      std::unique_ptr<Tree> toTree);

  /** Ensure that the overlay is tracking metadata for this inode
   * This is required whenever we are about to make a structural change
   * in the tree; renames, creation, deletion. */
  void materializeDirAndParents();

  /**
   * Internal API only for use by InodeMap.
   *
   * InodeMap will call this API when a child inode needs to be loaded.
   * The TreeInode will call InodeMap::inodeLoadComplete() or
   * InodeMap::inodeLoadFailed() when the load finishes.
   */
  void loadChildInode(PathComponentPiece name, fuse_ino_t number);

  /**
   * Unload all unreferenced children under this tree (recursively).
   *
   * This walks the children underneath this tree, unloading any inodes that
   * are unreferenced.
   */
  void unloadChildrenNow();

  /**
   * Load all materialized children underneath this TreeInode.
   *
   * This recursively descends into children directories.
   *
   * This method is intended to be called during the mount point initialization
   * to trigger loading of materialized inodes.  This allows other parts of the
   * code to assume that materialized inodes are always loaded once the mount
   * point has been initialized.
   *
   * Returns a Future that completes once all materialized inodes have been
   * loaded.
   */
  folly::Future<folly::Unit> loadMaterializedChildren();

  // Helper functions to be used only by CheckoutAction
  folly::Future<folly::Unit> checkoutReplaceEntry(
      CheckoutContext* ctx,
      InodePtr inode,
      const TreeEntry& newScmEntry);
  folly::Future<folly::Unit> checkoutRemoveChild(
      CheckoutContext* ctx,
      PathComponentPiece name,
      InodePtr inode);

 private:
  class TreeRenameLocks;
  class IncompleteInodeLoad;

  void registerInodeLoadComplete(
      folly::Future<std::unique_ptr<InodeBase>>& future,
      PathComponentPiece name,
      fuse_ino_t number);
  void inodeLoadComplete(
      PathComponentPiece childName,
      std::unique_ptr<InodeBase> childInode);

  folly::Future<std::unique_ptr<InodeBase>> startLoadingInodeNoThrow(
      Entry* entry,
      PathComponentPiece name,
      fuse_ino_t number) noexcept;

  folly::Future<std::unique_ptr<InodeBase>>
  startLoadingInode(Entry* entry, PathComponentPiece name, fuse_ino_t number);

  folly::Future<folly::Unit> doRename(
      TreeRenameLocks&& locks,
      PathComponentPiece srcName,
      PathMap<std::unique_ptr<Entry>>::iterator srcIter,
      TreeInodePtr destParent,
      PathComponentPiece destName);

  /** Translates a Tree object from our store into a Dir object
   * used to track the directory in the inode */
  static Dir buildDirFromTree(const Tree* tree);

  /**
   * Get a TreeInodePtr to ourself.
   *
   * This uses TreeInodePtr::newPtrFromExisting() internally.
   *
   * This should only be called in contexts where we know an external caller
   * already has an existing reference to us.  (Which is most places--a caller
   * has to have a reference to us in order to call any of our APIs.)
   */
  TreeInodePtr inodePtrFromThis() {
    return TreeInodePtr::newPtrFromExisting(this);
  }

  folly::Future<folly::Unit>
  rmdirImpl(PathComponent name, TreeInodePtr child, unsigned int attemptNum);

  /**
   * This helper function starts loading a currently unloaded child inode.
   * It must be held with the contents_ lock held.  (The Dir argument is only
   * required as a parameter to ensure that the caller is actually holding the
   * lock.)
   */
  folly::Future<InodePtr> loadChildLocked(
      Dir& dir,
      PathComponentPiece name,
      Entry* entry,
      std::vector<IncompleteInodeLoad>* pendingLoads);

  void computeCheckoutActions(
      CheckoutContext* ctx,
      const Tree* fromTree,
      const Tree* toTree,
      std::vector<std::unique_ptr<CheckoutAction>>* actions,
      std::vector<IncompleteInodeLoad>* pendingLoads);
  std::unique_ptr<CheckoutAction> processCheckoutEntry(
      CheckoutContext* ctx,
      Dir& contents,
      const TreeEntry* oldScmEntry,
      const TreeEntry* newScmEntry,
      std::vector<IncompleteInodeLoad>* pendingLoads);
  void saveOverlayPostCheckout(CheckoutContext* ctx, const Tree* tree);

  folly::Synchronized<Dir> contents_;
  /** Can be nullptr for the root inode only, otherwise will be non-null */
  TreeInode::Entry* entry_{nullptr};
};
}
}
