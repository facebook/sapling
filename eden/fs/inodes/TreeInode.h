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
#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Hash.h"
#include "eden/utils/PathMap.h"

namespace facebook {
namespace eden {

class CheckoutAction;
class CheckoutContext;
class DiffContext;
class EdenMount;
class FileHandle;
class GitIgnoreStack;
class InodeDiffCallback;
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
  enum : int { WRONG_TYPE_ERRNO = ENOTDIR };

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
    const folly::Optional<Hash>& getOptionalHash() const {
      return hash_;
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
    void setDematerialized(Hash hash) {
      hash_ = hash;
    }

    mode_t getMode() const {
      // Callers should not check getMode() if an inode is loaded.
      // If the child inode is loaded it is the authoritative source for
      // the mode bits.
      DCHECK(!inode);
      return mode;
    }

    /**
     * Check if the entry is a directory or not.
     *
     * It is okay for callers to call isDirectory() even if the inode is
     * loaded.  The file type for an existing entry never changes.
     */
    bool isDirectory() const;

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
      std::unique_ptr<Tree>&& tree);

  /// Construct an inode that only has backing in the Overlay area
  TreeInode(
      fuse_ino_t ino,
      TreeInodePtr parent,
      PathComponentPiece name,
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
   * Compute differences between a source control Tree and the current inode
   * state.
   *
   * @param context A pointer to the DiffContext containing parameters for the
   *     current diff operation.  The caller is responsible for ensuring that
   *     the DiffContext object remains valid until this diff completes.
   * @param currentPath The path to this Tree, as used for the purpose of diff
   *     computation.  Note that we do not block renames and other filesystem
   *     layout changes during diff operations, so this might not actually
   *     correspond to the current TreeInode's path.  However, it was the path
   *     that we used for computing ignored status, so we want to report diff
   *     results using this path.  Even if it may not currently be the
   *     TreeInode's path it reflects the path that used to be correct at some
   *     point since the diff started.
   * @param tree The source control Tree to compare the current state against.
   *     This may be null when comparing a portion of the file system tree that
   *     does not exist in source control.
   * @param parentIgnore A GitIgnoreStack containing the gitignore data for all
   *     parent directories of this one.  This parameter may be null if
   *     isIgnored is true.  The caller must ensure that this GitIgnoreStack
   *     object remains valid until the returned Future object completes.
   * @param isIgnored  Whether or not the current directory is ignored
   *     according to source control ignore rules.
   *
   * @return Returns a Future that will be fulfilled when the diff operation
   *     completes.  The caller must ensure that the InodeDiffCallback parameter
   *     remains valid until this Future completes.
   */
  folly::Future<folly::Unit> diff(
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::unique_ptr<Tree> tree,
      GitIgnoreStack* parentIgnore,
      bool isIgnored);

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
   *     This argument may be null if this path no longer exists in the
   *     destination commit.  If the destination location is empty, and this
   *     TreeInode is empty after the checkout operation (no untracked entries
   *     remain inside it), then this TreeInode itself will be unlinked as
   *     well.
   *
   * @return Returns a future that will be fulfilled once this tree and all of
   *     its children have been updated.
   */
  folly::Future<folly::Unit> checkout(
      CheckoutContext* ctx,
      std::unique_ptr<Tree> fromTree,
      std::unique_ptr<Tree> toTree);

  /**
   * Update this directory when a child entry is materialized.
   *
   * This will materialize this directory if it is not already materialized,
   * and will record that the child in question is materialized.
   *
   * This method should only be called by the child inode in question.
   *
   * With regards to specific implementation details of this API:
   * - The child inode must not be holding locks on itself when calling this
   *   method.  Typically the child updates its own in-memory state first, then
   *   releases its lock before calling childMaterialized() on its parent.
   * - The child should have written out its overlay data on disk before
   *   calling this method.  This ensures that the child always has overlay
   *   data on disk whenever its parent directory's overlay data indicates that
   *   the child is materialized.
   */
  void childMaterialized(
      const RenameLock& renameLock,
      PathComponentPiece childName,
      fuse_ino_t childNodeId);

  /**
   * Update this directory when a child entry is dematerialized.
   *
   * This method should only be called by the child inode in question.
   *
   * With regards to specific implementation details of this API:
   * - The child inode must not be holding locks on itself when calling this
   *   method.  Typically the child updates its own in-memory state first, then
   *   releases its lock before calling childMaterialized() on its parent.
   * - The child should delay removing its on-disk overlay state until after
   *   this method returns.  This ensures that the child always has overlay
   *   data on disk whenever its parent directory's overlay data indicates that
   *   the child is materialized.
   */
  void childDematerialized(
      const RenameLock& renameLock,
      PathComponentPiece childName,
      Hash childScmHash);

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

  /*
   * Update a tree entry as part of a checkout operation.
   *
   * This helper function is only to be used by CheckoutAction.
   *
   * @param ctx The CheckoutContext for the current checkout operation.
   *     The caller guarantees that the CheckoutContext argument will remain
   *     valid until the returned Future completes.
   * @param name The name of the child entry being replaced.
   * @param inode A pointer to the child InodeBase that is being updated.
   *     The path to this inode is guaranteed to match the name parameter.
   * @param oldTree If this entry referred to Tree in the source commit,
   *     then oldTree will be a pointer to its source control state.  oldTree
   *     will be null if this entry did not exist or if it referred to a Blob
   *     in the source commit.
   * @param newTree If this entry refers to Tree in the destination commit,
   *     then newTree will be a pointer to its source control state.  newTree
   *     will be null if this entry does not exist or if it refers to a Blob in
   *     the source commit.
   * @param newScmEntry The desired source control state for the new entry,
   *     or folly::none if the entry does not exist in the destination commit.
   *     This entry will refer to a tree if and only if the newTree parameter
   *     is non-null.
   */
  folly::Future<folly::Unit> checkoutUpdateEntry(
      CheckoutContext* ctx,
      PathComponentPiece name,
      InodePtr inode,
      std::unique_ptr<Tree> oldTree,
      std::unique_ptr<Tree> newTree,
      folly::Optional<TreeEntry> newScmEntry);

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

  /**
   * Materialize this directory in the overlay.
   *
   * This is required whenever we are about to make a structural change
   * in the tree; renames, creation, deletion.
   */
  void materialize(const RenameLock* renameLock = nullptr);

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

  /**
   * removeImpl() is the actual implementation used for unlink() and rmdir().
   *
   * The child inode in question must already be loaded.  removeImpl() will
   * confirm that this is still the correct inode for the given name, and
   * remove it if so.  If not it will attempt to load the child again, and will
   * retry the remove again (hence the attemptNum parameter).
   */
  template <typename InodePtrType>
  folly::Future<folly::Unit>
  removeImpl(PathComponent name, InodePtr child, unsigned int attemptNum);

  /**
   * tryRemoveChild() actually unlinks a child from our entry list.
   *
   * The caller must already be holding the mountpoint-wide RenameLock.
   *
   * This method also updates the overlay state if the child was removed
   * successfully.
   *
   * @param renameLock A reference to the rename lock (this parameter is
   *     required mostly to ensure that the caller is holding it).
   * @param name The entry name to remove.
   * @param child If this parameter is non-null, then only remove the entry if
   *     it refers to the specified inode.  If the entry does not refer to the
   *     inode in question, EBADF will be returned.
   *
   * @return Returns an errno value on error, or 0 on success.  Notable errors
   * include:
   * - ENOENT: no entry exists the specified name
   * - EBADF: An entry exists with the specified name, but the InodeBase object
   *   for it is not loaded, or it does not refer to the same inode as the
   *   child parameter (if child was non-null).
   * - EISDIR: the entry with the specified name is a directory (only returned
   *   if InodePtrType is FileInodePtr).
   * - ENOTDIR: the entry with the specified name is not a directory (only
   *   returned if InodePtrType is TreeInodePtr).
   * - ENOTEMPTY: the directory being removed is not empty.
   *
   * Callers should assume that tryRemoveChild() may still throw an exception
   * on other unexpected error cases.
   */
  template <typename InodePtrType>
  FOLLY_WARN_UNUSED_RESULT int tryRemoveChild(
      const RenameLock& renameLock,
      PathComponentPiece name,
      InodePtrType child);

  /**
   * checkPreRemove() is called by tryRemoveChild() for file or directory
   * specific checks before unlinking an entry.  Returns an errno value or 0.
   */
  static FOLLY_WARN_UNUSED_RESULT int checkPreRemove(const TreeInodePtr& child);
  static FOLLY_WARN_UNUSED_RESULT int checkPreRemove(const FileInodePtr& child);

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

  /**
   * Load the .gitignore file for this directory, then call computeDiff() once
   * it is loaded.
   */
  folly::Future<folly::Unit> loadGitIgnoreThenDiff(
      InodePtr gitignoreInode,
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::unique_ptr<Tree> tree,
      GitIgnoreStack* parentIgnore,
      bool isIgnored);

  /**
   * The bulk of the actual implementation of diff()
   *
   * The main diff() function's GitIgnoreStack parameter contains the ignore
   * data for the ancestors of this directory.  diff() loads .gitignore data
   * for the current directory and then invokes computeDiff() to perform the
   * diff once all .gitignore data is loaded.
   */
  folly::Future<folly::Unit> computeDiff(
      folly::Synchronized<Dir>::LockedPtr contentsLock,
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::unique_ptr<Tree> tree,
      std::unique_ptr<GitIgnoreStack> ignore,
      bool isIgnored);

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

  /**
   * Attempt to remove an empty directory during a checkout operation.
   *
   * Returns true on success, or false if the directory could not be removed.
   * The most likely cause of a failure is an ENOTEMPTY error if someone else
   * has already created a new file in a directory made empty by a checkout.
   */
  FOLLY_WARN_UNUSED_RESULT bool checkoutTryRemoveEmptyDir(CheckoutContext* ctx);

  folly::Synchronized<Dir> contents_;
};
}
}
