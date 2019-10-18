/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/File.h>
#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include <optional>
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeBase.h"

namespace facebook {
namespace eden {

class CheckoutAction;
class CheckoutContext;
class DiffContext;
class DirList;
class EdenMount;
class GitIgnoreStack;
class DiffCallback;
class InodeMap;
class ObjectStore;
class Overlay;
class RenameLock;
class Tree;
class TreeEntry;
class TreeInodeDebugInfo;
enum class InvalidationRequired : bool;

constexpr folly::StringPiece kDotEdenName{".eden"};

/**
 * The state of a TreeInode as held in memory.
 */
struct TreeInodeState {
  explicit TreeInodeState(DirContents&& dir, std::optional<Hash> hash)
      : entries{std::forward<DirContents>(dir)}, treeHash{hash} {}

  bool isMaterialized() const {
    return !treeHash.has_value();
  }
  void setMaterialized() {
    treeHash = std::nullopt;
  }

  DirContents entries;

  /**
   * If this TreeInode is unmaterialized (identical to an existing source
   * control Tree), treeHash contains the ID of the source control Tree
   * that this TreeInode is identical to.
   *
   * If this TreeInode is materialized (possibly modified from source
   * control, and backed by the Overlay instead of a source control Tree),
   * treeHash will be none.
   */
  std::optional<Hash> treeHash;
};

/**
 * Represents a directory in the file system.
 */
class TreeInode final : public InodeBaseMetadata<DirContents> {
 public:
  using Base = InodeBaseMetadata<DirContents>;

  enum : int { WRONG_TYPE_ERRNO = ENOTDIR };

  /**
   * Construct a TreeInode from a source control tree.
   */
  TreeInode(
      InodeNumber ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      mode_t initialMode,
      std::shared_ptr<const Tree>&& tree);

  /**
   * Construct an inode that only has backing in the Overlay area.
   */
  TreeInode(
      InodeNumber ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      mode_t initialMode,
      const std::optional<InodeTimestamps>& initialTimestamps,
      DirContents&& dir,
      std::optional<Hash> treeHash);

  /**
   * Construct the root TreeInode from a source control commit's root.
   */
  TreeInode(EdenMount* mount, std::shared_ptr<const Tree>&& tree);

  /**
   * Construct the root inode from data saved in the overlay.
   */
  TreeInode(EdenMount* mount, DirContents&& dir, std::optional<Hash> treeHash);

  ~TreeInode() override;

  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<Dispatcher::Attr> setattr(const fuse_setattr_in& attr) override;

  folly::Future<std::vector<std::string>> listxattr() override;
  folly::Future<std::string> getxattr(folly::StringPiece name) override;

  Dispatcher::Attr getAttrLocked(const DirContents& contents);

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

  InodeNumber getChildInodeNumber(PathComponentPiece name);

  FOLLY_NODISCARD folly::Future<folly::Unit> rename(
      PathComponentPiece name,
      TreeInodePtr newParent,
      PathComponentPiece newName);

  DirList readdir(DirList&& list, off_t off);

  const folly::Synchronized<TreeInodeState>& getContents() const {
    return contents_;
  }
  folly::Synchronized<TreeInodeState>& getContents() {
    return contents_;
  }

  FileInodePtr symlink(PathComponentPiece name, folly::StringPiece contents);

  TreeInodePtr mkdir(PathComponentPiece name, mode_t mode);
  FOLLY_NODISCARD folly::Future<folly::Unit> unlink(PathComponentPiece name);
  FOLLY_NODISCARD folly::Future<folly::Unit> rmdir(PathComponentPiece name);

  /**
   * Create a filesystem node.
   * Only unix domain sockets and regular files are supported; attempting to
   * create any other kind of node will fail.
   */
  FileInodePtr mknod(PathComponentPiece name, mode_t mode, dev_t rdev);

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
   *     completes.  The caller must ensure that the DiffCallback parameter
   *     remains valid until this Future completes.
   */
  folly::Future<folly::Unit> diff(
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::shared_ptr<const Tree> tree,
      const GitIgnoreStack* parentIgnore,
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
  FOLLY_NODISCARD folly::Future<folly::Unit> checkout(
      CheckoutContext* ctx,
      std::shared_ptr<const Tree> fromTree,
      std::shared_ptr<const Tree> toTree);

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
      PathComponentPiece childName);

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
  void loadChildInode(PathComponentPiece name, InodeNumber number);

  /**
   * Internal API only for use by InodeMap.
   *
   * InodeMap will this API when a child inode that has been unlinked
   * needs to be loaded.
   *
   * The TreeInode will call InodeMap::inodeLoadComplete() or
   * InodeMap::inodeLoadFailed() when the load finishes.
   */
  void loadUnlinkedChildInode(
      PathComponentPiece name,
      InodeNumber number,
      std::optional<Hash> hash,
      mode_t mode);

  /**
   * Unload all unreferenced children under this tree (recursively).
   *
   * This walks the children underneath this tree, unloading any inodes that
   * are unreferenced by Eden. If an inode is unreferenced by Eden but
   * still has a positive FUSE reference count, it will be unloaded and moved
   * into the InodeMap's unloadedInodes map.
   *
   * Returns the number of inodes unloaded.
   */
  size_t unloadChildrenNow();

  /**
   * Unload all children, recursively, neither referenced internally by Eden nor
   * by FUSE.
   *
   * Returns the number of inodes unloaded.
   */
  size_t unloadChildrenUnreferencedByFuse();

  /**
   * Unload all unreferenced inodes under this tree whose last access time is
   * older than the specified cutoff.
   *
   * Returns the number of inodes unloaded.
   */
  size_t unloadChildrenLastAccessedBefore(const timespec& cutoff);

  /*
   * Update a tree entry as part of a checkout operation.
   *
   * Returns whether or not the tree's contents were updated and the inode's
   * readdir cache must be flushed.
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
   *     or std::nullopt if the entry does not exist in the destination commit.
   *     This entry will refer to a tree if and only if the newTree parameter
   *     is non-null.
   */
  FOLLY_NODISCARD folly::Future<InvalidationRequired> checkoutUpdateEntry(
      CheckoutContext* ctx,
      PathComponentPiece name,
      InodePtr inode,
      std::shared_ptr<const Tree> oldTree,
      std::shared_ptr<const Tree> newTree,
      const std::optional<TreeEntry>& newScmEntry);

  /**
   * Get debug data about this TreeInode and all of its children (recursively).
   *
   * This populates the results argument with TreeInodeDebugInfo objects for
   * this TreeInode and all subdirectories inside of it.
   */
  void getDebugStatus(std::vector<TreeInodeDebugInfo>& results) const;

  /**
   * Returns a copy of this inode's metadata.
   */
  InodeMetadata getMetadata() const override;

 private:
  class TreeRenameLocks;
  class IncompleteInodeLoad;

  InodeMetadata getMetadataLocked(const DirContents&) const;

  /**
   * The InodeMap is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.
   */
  InodeMap* getInodeMap() const;

  /**
   * The ObjectStore is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.  (The ObjectStore is owned by the EdenMount.)
   */
  ObjectStore* getStore() const;

  void registerInodeLoadComplete(
      folly::Future<std::unique_ptr<InodeBase>>& future,
      PathComponentPiece name,
      InodeNumber number);
  void inodeLoadComplete(
      PathComponentPiece childName,
      std::unique_ptr<InodeBase> childInode);

  folly::Future<std::unique_ptr<InodeBase>> startLoadingInodeNoThrow(
      const DirEntry& entry,
      PathComponentPiece name) noexcept;

  folly::Future<std::unique_ptr<InodeBase>> startLoadingInode(
      const DirEntry& entry,
      PathComponentPiece name);

  /**
   * Materialize this directory in the overlay.
   *
   * This is required whenever we are about to make a structural change
   * in the tree; renames, creation, deletion.
   */
  void materialize(const RenameLock* renameLock = nullptr);

  FOLLY_NODISCARD folly::Future<folly::Unit> doRename(
      TreeRenameLocks&& locks,
      PathComponentPiece srcName,
      PathMap<DirEntry>::iterator srcIter,
      TreeInodePtr destParent,
      PathComponentPiece destName);

  Overlay* getOverlay() const;

  /**
   * Loads a tree from the overlay given an inode number.
   */
  std::optional<DirContents> loadOverlayDir(InodeNumber inodeNumber) const;

  /**
   * Saves the entries of this inode to the overlay.
   */
  void saveOverlayDir(const DirContents& contents) const;

  /**
   * Saves the entries for a specified inode number.
   */
  void saveOverlayDir(InodeNumber inodeNumber, const DirContents& contents)
      const;

  /**
   * Converts a Tree to a Dir and saves it to the Overlay under the given inode
   * number.
   */
  static DirContents
  saveDirFromTree(InodeNumber inodeNumber, const Tree* tree, EdenMount* mount);

  /** Translates a Tree object from our store into a Dir object
   * used to track the directory in the inode */
  static DirContents buildDirFromTree(const Tree* tree, Overlay* overlay);

  void updateAtime();

  void prefetch();

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
   * createImpl() is a helper function for creating new children inodes.
   *
   * This is used by create(), symlink(), and mknod().
   */
  FileInodePtr createImpl(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      PathComponentPiece name,
      mode_t mode,
      folly::ByteRange fileContents);

  /**
   * removeImpl() is the actual implementation used for unlink() and rmdir().
   *
   * The child inode in question must already be loaded.  removeImpl() will
   * confirm that this is still the correct inode for the given name, and
   * remove it if so.  If not it will attempt to load the child again, and will
   * retry the remove again (hence the attemptNum parameter).
   */
  template <typename InodePtrType>
  FOLLY_NODISCARD folly::Future<folly::Unit>
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
   * @param flushKernelCache This parameter indicates if we should tell the
   *     kernel to flush its cache for the removed entry.  This should always
   *     be set to true, unless tryRemoveChild() is being called from a FUSE
   *     unlink() or rmdir() call, in which case the kernel will update its
   *     cache automatically when the FUSE call returns.
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
  FOLLY_NODISCARD int tryRemoveChild(
      const RenameLock& renameLock,
      PathComponentPiece name,
      InodePtrType child,
      bool flushKernelCache);

  /**
   * checkPreRemove() is called by tryRemoveChild() for file or directory
   * specific checks before unlinking an entry.  Returns an errno value or 0.
   */
  FOLLY_NODISCARD static int checkPreRemove(const TreeInodePtr& child);
  FOLLY_NODISCARD static int checkPreRemove(const FileInodePtr& child);

  /**
   * This helper function starts loading a currently unloaded child inode.
   * It must be held with the contents_ lock held.  (The Dir argument is only
   * required as a parameter to ensure that the caller is actually holding the
   * lock.)
   */
  folly::Future<InodePtr> loadChildLocked(
      DirContents& dir,
      PathComponentPiece name,
      DirEntry& entry,
      std::vector<IncompleteInodeLoad>& pendingLoads);

  /**
   * Load the .gitignore file for this directory, then call computeDiff() once
   * it is loaded.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> loadGitIgnoreThenDiff(
      InodePtr gitignoreInode,
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::shared_ptr<const Tree> tree,
      const GitIgnoreStack* parentIgnore,
      bool isIgnored);

  /**
   * The bulk of the actual implementation of diff()
   *
   * The main diff() function's GitIgnoreStack parameter contains the ignore
   * data for the ancestors of this directory.  diff() loads .gitignore data
   * for the current directory and then invokes computeDiff() to perform the
   * diff once all .gitignore data is loaded.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> computeDiff(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      const DiffContext* context,
      RelativePathPiece currentPath,
      std::shared_ptr<const Tree> tree,
      std::unique_ptr<GitIgnoreStack> ignore,
      bool isIgnored);

  /**
   * Check to see if we can break out of a checkout() operation early.
   *
   * This should only be called for non-materialized TreeInodes that have a
   * source control hash.
   *
   * @param ctx The CheckoutContext
   * @param treeHash The source control hash for the TreeInode being updated.
   * @param fromTree The source control Tree that this checkout operation is
   *        moving away from.  This may be null if there was no source control
   *        state at this location previously.
   * @param toTree The destination source control Tree of the checkout.
   *        of the checkout).  This may be null if the destination state has no
   *        contents under this directory.
   */
  static bool canShortCircuitCheckout(
      CheckoutContext* ctx,
      const Hash& treeHash,
      const Tree* fromTree,
      const Tree* toTree);
  void computeCheckoutActions(
      CheckoutContext* ctx,
      const Tree* fromTree,
      const Tree* toTree,
      std::vector<std::unique_ptr<CheckoutAction>>& actions,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& wasDirectoryListModified);
  /**
   * Sets wasDirectoryListModified true if this checkout entry operation has
   * modified the directory contents, which implies the return value is nullptr.
   *
   * This function could return a std::variant of InvalidationRequired and
   * std::unique_ptr<CheckoutAction> instead of setting a boolean.
   */
  std::unique_ptr<CheckoutAction> processCheckoutEntry(
      CheckoutContext* ctx,
      DirContents& contents,
      const TreeEntry* oldScmEntry,
      const TreeEntry* newScmEntry,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& wasDirectoryListModified);
  void saveOverlayPostCheckout(CheckoutContext* ctx, const Tree* tree);

  /**
   * Send a request to the kernel to invalidate the pagecache for this inode,
   * which flushes the readdir cache. This is required when the child entry list
   * has changed. invalidateFuseEntryCache(name) only works if the entry name is
   * known to FUSE, which is not true for new entries.
   */
  void invalidateFuseInodeCache();

  /**
   * If running outside of a FUSE request (in which case the kernel already
   * knows to flush the appropriate caches), call invalidateFuseInodeCache().
   */
  void invalidateFuseInodeCacheIfRequired();

  /**
   * Send a request to the kernel to invalidate the dcache entry for the given
   * child entry name. The dcache caches name lookups to child inodes.
   *
   * This should be called when an entry is added, removed, or changed.
   * Invalidating upon removal is required because the kernel maintains a
   * negative cache on lookup failures.
   *
   * This is safe to call while holding the contents_ lock, but it is not
   * required.  Calling it without the contents_ lock held is preferable when
   * possible.
   */
  void invalidateFuseEntryCache(PathComponentPiece name);

  /**
   * Invalidate the kernel FUSE cache for this entry name only if we are not
   * being called from inside a FUSE request handler.
   *
   * If we are being invoked because of a FUSE request for this entry we don't
   * need to tell the kernel about the change--it will automatically know.
   */
  void invalidateFuseEntryCacheIfRequired(PathComponentPiece name);

  /**
   * Attempt to remove an empty directory during a checkout operation.
   *
   * Returns true on success, or false if the directory could not be removed.
   * The most likely cause of a failure is an ENOTEMPTY error if someone else
   * has already created a new file in a directory made empty by a checkout.
   */
  FOLLY_NODISCARD bool checkoutTryRemoveEmptyDir(CheckoutContext* ctx);

  folly::Synchronized<TreeInodeState> contents_;

  /**
   * Only prefetch blob metadata on the first readdir() of a loaded inode.
   */
  std::atomic<bool> prefetched_{false};
};

/**
 * An internal function which computes the difference between a Dir and a tree
 * as a set of strings starting with + and - followed by the entry name.
 */
std::optional<std::vector<std::string>> findEntryDifferences(
    const DirContents& dir,
    const Tree& tree);

} // namespace eden
} // namespace facebook
