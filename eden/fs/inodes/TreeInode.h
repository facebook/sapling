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
#include <folly/File.h>
#include <folly/Optional.h>
#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathMap.h"

namespace facebook {
namespace eden {

class CheckoutAction;
class CheckoutContext;
class DiffContext;
class EdenFileHandle;
class EdenMount;
class GitIgnoreStack;
class InodeDiffCallback;
class InodeMap;
class ObjectStore;
class Overlay;
class RenameLock;
class Tree;
class TreeEntry;
class TreeInodeDebugInfo;

constexpr folly::StringPiece kDotEdenName{".eden"};

/**
 * Represents a directory in the file system.
 */
class TreeInode : public InodeBase {
 public:
  enum : int { WRONG_TYPE_ERRNO = ENOTDIR };

  enum class Recurse {
    SHALLOW,
    DEEP,
  };

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
  class Entry {
   public:
    /**
     * Create a hash for a non-materialized entry.
     */
    Entry(mode_t m, InodeNumber number, Hash hash)
        : mode_{m}, hash_{hash}, inodeNumber_{number} {
      DCHECK(number.hasValue());
    }

    /**
     * Create a hash for a materialized entry.
     */
    Entry(mode_t m, InodeNumber number) : mode_{m}, inodeNumber_{number} {
      DCHECK(number.hasValue());
    }

    Entry(Entry&& e) = default;
    Entry& operator=(Entry&& e) = default;
    Entry(const Entry& e) = delete;
    Entry& operator=(const Entry& e) = delete;

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
      DCHECK(hash_.hasValue());
      return hash_.value();
    }

    const folly::Optional<Hash>& getOptionalHash() const {
      return hash_;
    }

    InodeNumber getInodeNumber() const {
      return inodeNumber_;
    }

    void setMaterialized() {
      hash_.clear();
    }

    void setDematerialized(Hash hash) {
      DCHECK(inode_);
      hash_ = hash;
    }

    mode_t getMode() const {
      // Callers should not check getMode() if an inode is loaded.
      // If the child inode is loaded it is the authoritative source for
      // the mode bits.
      DCHECK(!inode_);
      return mode_;
    }

    mode_t getModeUnsafe() const {
      // TODO: T20354866 Remove this method once all callers are refactored.
      //
      // Callers should always call getMode() instead. This method only exists
      // for supporting legacy code which will be refactored eventually.
      return mode_;
    }

    /**
     * Get the file type, as a dtype_t value as used by readdir()
     *
     * It is okay for callers to call getDtype() even if the inode is
     * loaded.  The file type for an existing entry never changes.
     */
    dtype_t getDtype() const {
      return mode_to_dtype(mode_);
    }

    /**
     * Check if the entry is a directory or not.
     *
     * It is okay for callers to call isDirectory() even if the inode is
     * loaded.  The file type for an existing entry never changes.
     */
    bool isDirectory() const {
      return dtype_t::Dir == getDtype();
    }

    InodeBase* getInode() const {
      return inode_;
    }

    InodePtr getInodePtr() const {
      // It's safe to call newPtrLocked because calling getInode() implies the
      // TreeInode's contents_ lock is held.
      return inode_ ? InodePtr::newPtrLocked(inode_) : InodePtr{};
    }

    /**
     * Same as getInodePtr().asFilePtrOrNull() except it avoids constructing
     * a FileInodePtr if the entry does not point to a FileInode.
     */
    FileInodePtr asFilePtrOrNull() const;

    /**
     * Same as getInodePtr().asTreePtrOrNull() except it avoids constructing
     * a TreeInodePtr if the entry does not point to a FileInode.
     */
    TreeInodePtr asTreePtrOrNull() const;

    void setInode(InodeBase* inode) {
      DCHECK(!inode_);
      DCHECK(inode);
      DCHECK_EQ(inodeNumber_, inode->getNodeId());
      inode_ = inode;
    }

    void clearInode() {
      DCHECK(inode_);
      inode_ = nullptr;
    }

   private:
    /**
     * The initial entry type for this entry.
     */
    mode_t mode_{0};

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
     * The inode number assigned to this entry.  Is never zero.
     */
    InodeNumber inodeNumber_{};

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
    InodeBase* inode_{nullptr};
  };

  // TODO: We can do better than this. When mode_t is stored in the InodeTable,
  // an entry can be in one of two states:
  // 1. Non-materialized, where we only need to store
  //    (TreeEntryType, Hash, InodeNumber)
  // 2. Materialized, where hash is unset and inode_ is non-null.
  // I think that could fit in 32 bytes, which would be a material savings
  // given how many trees Eden tends to keep loaded.
  static_assert(sizeof(Entry) == 48, "Entry is six words");

  /** Represents a directory in the overlay */
  struct Dir {
    /** The direct children of this directory */
    PathMap<Entry> entries;
    InodeTimestamps timeStamps;
    /**
     * If this TreeInode is unmaterialized (identical to an existing source
     * control Tree), treeHash contains the ID of the source control Tree
     * that this TreeInode is identical to.
     *
     * If this TreeInode is materialized (possibly modified from source
     * control, and backed by the Overlay instead of a source control Tree),
     * treeHash will be none.
     */
    folly::Optional<Hash> treeHash;

    bool isMaterialized() const {
      return !treeHash.hasValue();
    }
    void setMaterialized() {
      treeHash = folly::none;
    }
  };

  /** Holds the results of a create operation. */
  struct CreateResult {
    /// file attributes and cache ttls.
    Dispatcher::Attr attr;
    /// The newly created inode instance.
    InodePtr inode;
    /// The newly opened file handle.
    std::shared_ptr<EdenFileHandle> file;

    explicit CreateResult(const EdenMount* mount);
  };

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
      Dir&& dir);

  /**
   * Construct the root TreeInode from a source control commit's root.
   */
  TreeInode(EdenMount* mount, std::shared_ptr<const Tree>&& tree);
  TreeInode(EdenMount* mount, Dir&& tree);

  ~TreeInode() override;

  folly::Future<Dispatcher::Attr> getattr() override;
  folly::Future<folly::Unit> prefetch() override;
  void updateOverlayHeader() override;
  Dispatcher::Attr getAttrLocked(const Dir* contents);

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

  std::shared_ptr<DirHandle> opendir();
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

  folly::Future<CreateResult>
  create(PathComponentPiece name, mode_t mode, int flags);
  FileInodePtr symlink(PathComponentPiece name, folly::StringPiece contents);

  TreeInodePtr mkdir(PathComponentPiece name, mode_t mode);
  folly::Future<folly::Unit> unlink(PathComponentPiece name);
  folly::Future<folly::Unit> rmdir(PathComponentPiece name);

  /**
   * Create a special filesystem node.
   * Only unix domain sockets are supported; attempting to create any
   * other kind of node will fail.
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
   *     completes.  The caller must ensure that the InodeDiffCallback parameter
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
  folly::Future<folly::Unit> checkout(
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
      folly::Optional<Hash> hash,
      mode_t mode);

  /**
   * Unload all unreferenced children under this tree (recursively).
   *
   * This walks the children underneath this tree, unloading any inodes that
   * are unreferenced.
   */
  void unloadChildrenNow();

  /**
   * Unload all unreferenced inodes under this tree whose last access time is
   * older than the specified cutoff.
   *
   * Returns the number of inodes unloaded.
   */
  uint64_t unloadChildrenLastAccessedBefore(const timespec& cutoff);

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
  folly::Future<folly::Unit> loadMaterializedChildren(
      Recurse recurse = Recurse::DEEP);

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
      std::shared_ptr<const Tree> oldTree,
      std::shared_ptr<const Tree> newTree,
      const folly::Optional<TreeEntry>& newScmEntry);

  /**
   * Get debug data about this TreeInode and all of its children (recursively).
   *
   * This populates the results argument with TreeInodeDebugInfo objects for
   * this TreeInode and all subdirectories inside of it.
   */
  void getDebugStatus(std::vector<TreeInodeDebugInfo>& results) const;

  /**
   * Get the timestamps of the inode.
   */
  InodeTimestamps getTimestamps() const;

  /**
   * Helper function to set the atime of a TreeInode. In order to set atime of a
   * file in TreeInodeDirHandle::readdir which doesnot  have access to.
   * TreeInode::contents_ we have this function. This has to be public since we
   * are using it TreeInodeDirHandle class.
   */
  void updateAtimeToNow();

 private:
  class TreeRenameLocks;
  class IncompleteInodeLoad;

  void registerInodeLoadComplete(
      folly::Future<std::unique_ptr<InodeBase>>& future,
      PathComponentPiece name,
      InodeNumber number);
  void inodeLoadComplete(
      PathComponentPiece childName,
      std::unique_ptr<InodeBase> childInode);

  folly::Future<std::unique_ptr<InodeBase>> startLoadingInodeNoThrow(
      const Entry& entry,
      PathComponentPiece name) noexcept;

  folly::Future<std::unique_ptr<InodeBase>> startLoadingInode(
      const Entry& entry,
      PathComponentPiece name);

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
      PathMap<Entry>::iterator srcIter,
      TreeInodePtr destParent,
      PathComponentPiece destName);

  Overlay* getOverlay() const;

  /**
   * Loads a tree from the overlay given an inode number.
   */
  folly::Optional<Dir> loadOverlayDir(InodeNumber inodeNumber) const;

  /**
   * Saves the entries of this inode to the overlay.
   */
  void saveOverlayDir(const Dir& contents) const;

  /**
   * Saves the entries for a specified inode number.
   */
  void saveOverlayDir(InodeNumber inodeNumber, const Dir& contents) const;

  /**
   * Converts a Tree to a Dir and saves it to the Overlay under the given inode
   * number.
   */
  static Dir saveDirFromTree(
    InodeNumber inodeNumber,
    const Tree* tree,
    EdenMount* mount);

  /** Translates a Tree object from our store into a Dir object
   * used to track the directory in the inode */
  static Dir buildDirFromTree(
      const Tree* tree,
      const struct timespec& lastCheckoutTime,
      InodeMap* inodeMap);

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
   *
   * If outHandle is non-null a FileHandle will also be created and will be
   * returned via this parameter.
   */
  FileInodePtr createImpl(
      folly::Synchronized<Dir>::LockedPtr contentsLock,
      PathComponentPiece name,
      mode_t mode,
      folly::ByteRange fileContents,
      std::shared_ptr<EdenFileHandle>* outHandle);

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
      Dir& dir,
      PathComponentPiece name,
      Entry& entry,
      std::vector<IncompleteInodeLoad>* pendingLoads);

  /**
   * Load the .gitignore file for this directory, then call computeDiff() once
   * it is loaded.
   */
  folly::Future<folly::Unit> loadGitIgnoreThenDiff(
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
  folly::Future<folly::Unit> computeDiff(
      folly::Synchronized<Dir>::LockedPtr contentsLock,
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
   * Send a request to the kernel to invalidate the FUSE cache for the given
   * child entry name.
   *
   * This is safe to call while holding the contents_ lock, but it is not
   * required.  Calling it without the contents_ lock held is preferable when
   * possible.
   */
  void invalidateFuseCache(PathComponentPiece name);

  /**
   * Invalidate the kernel FUSE cache for this entry name only if we are not
   * being called from inside a FUSE request handler.
   *
   * If we are being invoked because of a FUSE request for this entry we don't
   * need to tell the kernel about the change--it will automatically know.
   */
  void invalidateFuseCacheIfRequired(PathComponentPiece name);

  /**
   * Attempt to remove an empty directory during a checkout operation.
   *
   * Returns true on success, or false if the directory could not be removed.
   * The most likely cause of a failure is an ENOTEMPTY error if someone else
   * has already created a new file in a directory made empty by a checkout.
   */
  FOLLY_NODISCARD bool checkoutTryRemoveEmptyDir(CheckoutContext* ctx);

  /**
   * Helper function called inside InodeBase::setattr to perform TreeInode
   * specific operation during setattr.
   */
  folly::Future<Dispatcher::Attr> setInodeAttr(
      const fuse_setattr_in& attr) override;

  folly::Synchronized<Dir> contents_;
};

/**
 * An internal function which computes the difference between a Dir and a tree
 * as a set of strings starting with + and - followed by the entry name.
 */
folly::Optional<std::vector<std::string>> findEntryDifferences(
    const TreeInode::Dir& dir,
    const Tree& tree);

} // namespace eden
} // namespace facebook
