/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/CancellationToken.h>
#include <folly/CppAttributes.h>
#include <folly/File.h>
#include <folly/Function.h>
#include <folly/Portability.h>
#include <folly/Synchronized.h>
#include <folly/coro/safe/NowTask.h>
#include <chrono>
#include <memory>
#include <optional>
#include <vector>
#include "eden/common/utils/FileOffset.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/fuse/Invalidation.h"
#include "eden/fs/inodes/CheckoutAction.h"
#include "eden/fs/inodes/DirEntry.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/Traverse.h"
#include "eden/fs/model/EntryAttributeFlags.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxDataFwd.h"
#include "eden/fs/utils/MiniTracer.h"

namespace facebook::eden {

class CheckoutAction;
class CheckoutContext;
class MiniTracer;
class DeferredDiffEntry;
class DiffContext;
class FuseDirList;
class NfsDirList;
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
class PrjfsDirEntry;
#ifdef _WIN32
class PrjfsChannel;
#endif
class VirtualInode;
#ifndef _WIN32
struct GcBarrierTrie;
#endif

constexpr folly::StringPiece kDotEdenName{".eden"};

/**
 * The state of a TreeInode as held in memory.
 */
struct TreeInodeState {
  explicit TreeInodeState(DirContents&& dir, std::optional<ObjectId> id)
      : entries{std::forward<DirContents>(dir)}, treeId{std::move(id)} {}

  bool isMaterialized() const {
    return !treeId.has_value();
  }
  void setMaterialized() {
    treeId = std::nullopt;
  }

  DirContents entries;

  /**
   * If this TreeInode is unmaterialized (identical to an existing source
   * control Tree), treeId contains the ID of the source control Tree
   * that this TreeInode is identical to.
   *
   * If this TreeInode is materialized (possibly modified from source
   * control, and backed by the Overlay instead of a source control Tree),
   * treeId will be none.
   */
  std::optional<ObjectId> treeId;
};

/**
 * Represents a directory in the file system.
 */
class TreeInode final : public InodeBaseMetadata<DirContents> {
 public:
  using Base = InodeBaseMetadata<DirContents>;

  enum : int { WRONG_TYPE_ERRNO = ENOTDIR };

  enum class InitialPermissionCheck {
    Unknown,
    JustDenied,
  };

  /**
   * Construct a TreeInode from an unrestricted source control tree.
   */
  TreeInode(
      InodeNumber ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      mode_t initialMode,
      std::shared_ptr<const Tree>&& tree);

  /**
   * Construct an inode that only has backing in the Overlay area.
   * Set isRestricted=true for directories denied by server-side path ACLs.
   */
  TreeInode(
      InodeNumber ino,
      TreeInodePtr parent,
      PathComponentPiece name,
      mode_t initialMode,
      const std::optional<InodeTimestamps>& initialTimestamps,
      DirContents&& dir,
      std::optional<ObjectId> treeId,
      bool isRestricted = false,
      std::optional<bool> hasACL = std::nullopt,
      InitialPermissionCheck initialPermissionCheck =
          InitialPermissionCheck::Unknown);

  /**
   * Construct the root TreeInode from a source control commit's root.
   */
  TreeInode(EdenMount* mount, std::shared_ptr<const Tree>&& tree);

  /**
   * Construct the root inode from data saved in the overlay.
   */
  TreeInode(
      EdenMount* mount,
      DirContents&& dir,
      const std::optional<ObjectId>& treeId,
      bool isRestricted = false,
      std::optional<bool> hasACL = std::nullopt,
      InitialPermissionCheck initialPermissionCheck =
          InitialPermissionCheck::Unknown);

  ~TreeInode() override;

  ImmediateFuture<struct stat> stat(
      const ObjectFetchContextPtr& context) override;

  folly::coro::now_task<struct stat> co_stat(
      const ObjectFetchContextPtr& context) override;

  ImmediateFuture<struct stat> setattr(
      const DesiredMetadata& desired,
      const ObjectFetchContextPtr& fetchContext) override;

#ifndef _WIN32
  ImmediateFuture<std::vector<std::string>> listxattr() override;
  ImmediateFuture<std::string> getxattr(
      folly::StringPiece name,
      const ObjectFetchContextPtr& context) override;
#endif // !_WIN32

  /**
   * Get the inode object for a child of this directory.
   *
   * Implements getOrLoadChild if loadInodes is true. If loadInodes is false and
   * the Inode load is already-in-progress, this may NOT return the loading
   * inode. Otherwise, the returned VirtualInode may contain a ObjectStore
   * Tree or a DirEntry/TreeEntry representing the entry.
   */
  ImmediateFuture<VirtualInode> getOrFindChild(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool loadInodes);

  /**
   * Coroutine version of getOrFindChild.
   */
  folly::coro::now_task<VirtualInode> co_getOrFindChild(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool loadInodes);

  /**
   * Retrieves VirtualInode for each of entry in this Tree, like
   * getOrFindChild, but for all the children of a directory.
   *
   * Note that this is separated out from the readdir logic below. There are
   * a few reasons for this. First. this method will not return information
   * about the . or .. entries in a directory. It only returns the contained
   * files and directories. Second, this method does not take an offset. Only
   * the entire directory can be listed. The readdir logic is complicated by
   * these two requirements, so we choose to use a much simpler implementation
   * here.
   *
   * Implements getOrLoadChild if loadInodes is true. If loadInodes is false and
   * the inode load is already-in-progress, this may NOT return the loading
   * inode. Otherwise, the returned VirtualInode may contain a ObjectStore
   * Tree or a DirEntry/TreeEntry representing the entry.
   */
  std::vector<std::pair<PathComponent, ImmediateFuture<VirtualInode>>>
  getChildren(const ObjectFetchContextPtr& context, bool loadInodes);

  /**
   * Coroutine variant of getChildren() returning eagerly-resolved
   * Try<VirtualInode> values instead of per-entry ImmediateFutures.
   */
  folly::coro::now_task<
      std::vector<std::pair<PathComponent, folly::Try<VirtualInode>>>>
  co_getChildren(const ObjectFetchContextPtr& context, bool loadInodes = false);

  /**
   * Pipelined coroutine version of getChildren + getEntryAttributes.
   *
   * Each child task does (resolve VirtualInode → fetch attributes) in
   * sequence, and all child tasks run in parallel under a single
   * collectAllTryRange. This avoids the barrier between phases that a
   * separate co_getChildren followed by per-attr tasks would impose,
   * preserving the latency profile of the original futures-based
   * implementation while still avoiding the ImmediateFuture wrapper
   * overhead.
   */
  folly::coro::now_task<
      std::vector<std::pair<PathComponent, folly::Try<EntryAttributes>>>>
  co_getChildrenAttributes(
      EntryAttributeFlags requestedAttributes,
      RelativePath path,
      const std::shared_ptr<ObjectStore>& objectStore,
      timespec lastCheckoutTime,
      const ObjectFetchContextPtr& context,
      std::optional<bool> ancestorUnderAcl = std::nullopt);

  /**
   * Get the inode object for a child of this directory.
   *
   * The Inode object will be loaded if it is not already loaded.
   */
  ImmediateFuture<InodePtr> getOrLoadChild(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context);
  folly::coro::now_task<InodePtr> co_getOrLoadChild(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context);
  ImmediateFuture<TreeInodePtr> getOrLoadChildTree(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context);
  folly::coro::now_task<TreeInodePtr> co_getOrLoadChildTree(
      PathComponentPiece name,
      const ObjectFetchContextPtr& context);

  /**
   * Recursively look up a child inode.
   *
   * The Inode object in question, and all intervening TreeInode objects,
   * will be loaded if they are not already loaded.
   */
  ImmediateFuture<InodePtr> getChildRecursive(
      RelativePathPiece name,
      const ObjectFetchContextPtr& context);

  folly::coro::now_task<InodePtr> co_getChildRecursive(
      RelativePathPiece name,
      const ObjectFetchContextPtr& context);

  InodeNumber getChildInodeNumber(PathComponentPiece name);

  [[nodiscard]] ImmediateFuture<folly::Unit> rename(
      PathComponentPiece name,
      TreeInodePtr newParent,
      PathComponentPiece newName,
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context);

#ifndef _WIN32
  FuseDirList fuseReaddir(
      FuseDirList&& list,
      FileOffset off,
      const ObjectFetchContextPtr& context);
#endif

  /**
   * Populate the list with as many directory entries as possible starting from
   * the inode start.
   *
   * Return the filled directory list as well as a boolean indicating if the
   * listing is complete.
   */
  std::tuple<NfsDirList, bool> nfsReaddir(
      NfsDirList&& list,
      off_t off,
      const ObjectFetchContextPtr& context);

  /**
   * Acquire a read lock on the contents.
   * Calls checkAccess() to enforce path ACL restrictions.
   */
  folly::Synchronized<TreeInodeState>::ConstLockedPtr lockContentsRead() const {
    checkAccess();
    return contents_.rlock();
  }

  /**
   * Acquire a write lock on the contents.
   * Calls checkAccess() to enforce path ACL restrictions.
   */
  folly::Synchronized<TreeInodeState>::LockedPtr lockContentsWrite() {
    checkAccess();
    return contents_.wlock();
  }

  /**
   * Direct access to contents without ACL checks. Only for internal
   * operations that must bypass restrictions (inode unload, checkout).
   * Prefer lockContentsRead() / lockContentsWrite() for normal access.
   */
  const folly::Synchronized<TreeInodeState>& getContentsUnchecked() const {
    return contents_;
  }
  folly::Synchronized<TreeInodeState>& getContentsUnchecked() {
    return contents_;
  }

  /**
   * Return a copy of all child entry names. This acquires and releases the
   * contents lock internally.
   */
  std::vector<PathComponent> getChildNames() const;

  /**
   * Snapshot of directory entries and tree ID for traversal purposes.
   * Copies entry metadata so the caller can use it without holding
   * the contents lock.
   */
  struct TraversalSnapshot {
    std::vector<ChildEntry> children;
    std::optional<ObjectId> treeId;
  };
  TraversalSnapshot getTraversalSnapshot() const;

  std::optional<ObjectId> getObjectId() const override;

  // Bypasses checkAccess(): all user-facing callers are guarded upstream
  // (path traversal, lock accessors). Internal callers (InodeMap, GC)
  // need this to work on restricted inodes.
  bool isMaterialized() const override {
    return contents_.rlock()->isMaterialized();
  }

  /**
   * Get the digest id for this inode.
   *
   * DEPRECATED: Use co_getDigestHash() instead.
   */
  ImmediateFuture<std::optional<Hash32>> getDigestHash(
      const ObjectFetchContextPtr& fetchContext);

  folly::coro::now_task<std::optional<Hash32>> co_getDigestHash(
      const ObjectFetchContextPtr& fetchContext);

  /**
   * Get the digest size for this inode.
   */
  ImmediateFuture<std::optional<uint64_t>> getDigestSize(
      const ObjectFetchContextPtr& fetchContext);

  /**
   * Get the tree aux data for this inode.
   */
  ImmediateFuture<std::optional<TreeAuxData>> getTreeAuxData(
      const ObjectFetchContextPtr& fetchContext);

  folly::coro::now_task<std::optional<TreeAuxData>> co_getTreeAuxData(
      const ObjectFetchContextPtr& fetchContext);

  FileInodePtr symlink(
      PathComponentPiece name,
      folly::StringPiece contents,
      InvalidationRequired invalidate);

  TreeInodePtr
  mkdir(PathComponentPiece name, mode_t mode, InvalidationRequired invalidate);
  [[nodiscard]] ImmediateFuture<folly::Unit> unlink(
      PathComponentPiece name,
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context);
  [[nodiscard]] ImmediateFuture<folly::Unit> rmdir(
      PathComponentPiece name,
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context);

  /**
   * Remove the file or directory starting at name.
   *
   * In the case where name is a directory, this does a recursive removal of
   * all of its children too. This method ensures the invalidation is flushed
   * so the caller would see the up-to-date state after when call is finished.
   *
   * Note that this may fail if a concurrent file/directory creation is being
   * performed in that hierarchy. The caller is responsible for handling this
   * and potentially calling this function again.
   */
  ImmediateFuture<folly::Unit> removeRecursively(
      PathComponentPiece name,
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context);

  /**
   * Internal method intended for removeRecursively to use. This method does
   * not flush invalidation so the caller won't see the up-to-date content
   * after return. Call EdenMount::flushInvalidations to ensure any changes to
   * the inode will be visible after it returns.
   */
  ImmediateFuture<folly::Unit> removeRecursivelyNoFlushInvalidation(
      PathComponentPiece name,
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context);

  /**
   * Attempts to remove and unlink children of this inode. Under concurrent
   * modification, it is not guaranteed that TreeInode is empty after this
   * function returns.
   */
  void removeAllChildrenRecursively(
      InvalidationRequired invalidate,
      const ObjectFetchContextPtr& context,
      const RenameLock& renameLock);

  /**
   * For unloaded nodes, the removal should be simpler: remove the node
   * from entries and update the overlay.
   * If the return value is valid, the entry was not removed, and the child's
   * loaded inode was returned.
   */
  InodePtr tryRemoveUnloadedChild(
      PathComponentPiece name,
      InvalidationRequired invalidate);

  /**
   * Create a filesystem node.
   * Only unix domain sockets and regular files are supported; attempting to
   * create any other kind of node will fail.
   */
  FileInodePtr mknod(
      PathComponentPiece name,
      mode_t mode,
      dev_t rdev,
      InvalidationRequired invalidate);

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
   * @return Returns an ImmediateFuture that will be fulfilled when the diff
   *     operation completes. The caller must ensure that the DiffCallback
   *     parameter remains valid until this Future completes.
   */
  ImmediateFuture<folly::Unit> diff(
      DiffContext* context,
      RelativePathPiece currentPath,
      std::vector<std::shared_ptr<const Tree>> trees,
      const GitIgnoreStack* parentIgnore,
      bool isIgnored);

  folly::coro::now_task<folly::Unit> co_diff(
      DiffContext* context,
      RelativePathPiece currentPath,
      std::vector<std::shared_ptr<const Tree>> trees,
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
   *     destination commit.  This tree inode will not be unlinked even if
   *     toTree is null. The caller is responsible for unlinking if necessary.
   *
   * @return Returns a future that will be fulfilled once this tree and all of
   *     its children have been updated.
   */
  [[nodiscard]] ImmediateFuture<CheckoutSubtreeResult> checkout(
      CheckoutContext* ctx,
      std::shared_ptr<const Tree> fromTree,
      std::shared_ptr<const Tree> toTree,
      bool reportLocalOnlyAsConflicts = false);

  [[nodiscard]] folly::coro::now_task<CheckoutSubtreeResult> co_checkout(
      CheckoutContext* ctx,
      std::shared_ptr<const Tree> fromTree,
      std::shared_ptr<const Tree> toTree,
      bool reportLocalOnlyAsConflicts = false);

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
      bool writeOverlay = true);

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
      ObjectId childScmId,
      bool writeOverlay = true,
      bool isRestricted = false,
      std::optional<bool> hasACL = std::nullopt);

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
      std::optional<ObjectId> id,
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
   * Unload all children, recursively, neither referenced internally by Eden
   * nor by FUSE or ProjectedFS.
   *
   * Returns the number of inodes unloaded.
   */
  size_t unloadChildrenUnreferencedByFs();

#ifndef _WIN32
  /**
   * Unload all unreferenced inodes under this tree whose last access time is
   * older than the specified cutoff.
   *
   * Returns the number of inodes unloaded.
   */
  size_t unloadChildrenLastAccessedBefore(const timespec& cutoff);
#endif

  /**
   * The first step of inode garbage collection.
   * This step behaves differently depending on the platform:
   * - On Linux, FUSE decreases the FS ref couunt by itself. Therefore we don't
   *   need to decrease FS ref count manually. We only unload not recently used
   *   inodes here
   * - On Windows and macOS, it recursively collects all child inodes,
   *   then decreases the filesystem reference count to zero for inodes
   *   that have not been accessed since the specified cutoff time.
   * This process is bottom-up recursive: if a child inode is retained,
   * all of its parent inodes are also retained, regardless of their access
   * time.
   */
  ImmediateFuture<uint64_t /* numInvalidated */>
  handleChildrenNotAccessedRecently(
      std::chrono::system_clock::time_point cutoff,
      const ObjectFetchContextPtr& context,
      folly::CancellationToken cancellationToken = {});

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
  [[nodiscard]] ImmediateFuture<CheckoutActionResult> checkoutUpdateEntry(
      CheckoutContext* ctx,
      PathComponentPiece name,
      InodePtr inode,
      std::shared_ptr<const Tree> oldTree,
      std::shared_ptr<const Tree> newTree,
      const std::optional<Tree::value_type>& newScmEntry);

  [[nodiscard]] folly::coro::now_task<CheckoutActionResult>
  co_checkoutUpdateEntry(
      CheckoutContext* ctx,
      PathComponentPiece name,
      InodePtr inode,
      std::shared_ptr<const Tree> oldTree,
      std::shared_ptr<const Tree> newTree,
      const std::optional<Tree::value_type>& newScmEntry);

  /**
   * Returns a copy of this inode's metadata.
   */
#ifndef _WIN32
  InodeMetadata getMetadata() const override;
#endif

  void forceMetadataUpdate() override;

#ifndef _WIN32
  ImmediateFuture<folly::Unit> ensureMaterialized(
      const ObjectFetchContextPtr& fetchContext,
      bool followSymlink) override;
#endif

  /**
   * Called by the child's inode (while it's location lock is held!)
   */
  void childWasStat(bool isFile, const ObjectFetchContext& context);

  uint64_t getInMemoryDescendants();

  void increaseInMemoryDescendants(int64_t inc);

 private:
  class TreeRenameLocks;
  class IncompleteInodeLoad;
  struct CheckoutSetup;
  struct CheckoutFinalizeState;
  struct RestrictionTransitionPrep;
  struct DirectoryRemovalResult;

#ifndef _WIN32
  InodeMetadata getMetadataLocked(const DirContents&) const;
#endif

  struct stat statWithCurrentRestrictionState() const;

  CheckoutSetup beginCheckout(
      CheckoutContext* ctx,
      const std::shared_ptr<const Tree>& fromTree,
      const std::shared_ptr<const Tree>& toTree,
      bool reportLocalOnlyAsConflicts);

  folly::Try<CheckoutFinalizeState> processCheckoutActionResults(
      CheckoutContext* ctx,
      const std::vector<std::shared_ptr<CheckoutAction>>& actions,
      bool shouldInvalidateDirectory,
      bool propagateErrors,
      bool hadConflicts,
      std::vector<folly::Try<CheckoutActionResult>>& actionResults);

  // Synchronous helpers for checkoutUpdateEntry. The caller must hold the
  // CheckoutContext's rename lock.
  [[nodiscard]] CheckoutActionResult replaceFileEntry(
      CheckoutContext* ctx,
      PathComponentPiece name,
      const InodePtr& inode,
      const std::optional<Tree::value_type>& newScmEntry);

  // Returns false without modifying the inode if it is not a valid empty
  // restricted placeholder.
  [[nodiscard]] bool updateRestrictedPlaceholder(
      const ObjectId& treeId,
      AclRootState aclRootState);

  RestrictionTransitionPrep prepareRestrictionTransition(
      CheckoutContext* ctx,
      const TreeInodePtr& treeInode,
      const Tree::value_type& replacementEntry,
      bool newRestricted);

  [[nodiscard]] CheckoutActionResult finalizeRestrictionTransition(
      CheckoutContext* ctx,
      const TreeInodePtr& treeInode,
      PathComponentPiece currentName,
      bool newRestricted,
      CheckoutSubtreeResult result);

  DirectoryRemovalResult finalizeDirectoryRemoval(
      CheckoutContext* ctx,
      const TreeInodePtr& treeInode,
      std::shared_ptr<const Tree> newTree,
      const std::optional<Tree::value_type>& newScmEntry,
      PathComponentPiece localName,
      bool hadConflicts);

  /**
   * The InodeMap is guaranteed to remain valid for at least the lifetime of
   * the TreeInode object.
   */
  InodeMap* getInodeMap() const;
  std::weak_ptr<InodeMap> getInodeMapWeak() const;

  void registerInodeLoadComplete(
      folly::Future<std::unique_ptr<InodeBase>>& future,
      PathComponentPiece name,
      InodeNumber number);
  void inodeLoadComplete(
      PathComponentPiece childName,
      std::unique_ptr<InodeBase> childInode);

  /**
   * Start loading inode for name. If async=true, potentially blocking
   * operations are forced to run through the executor (async).
   */
  folly::Future<std::unique_ptr<InodeBase>> startLoadingInodeNoThrow(
      const DirEntry& entry,
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool async) noexcept;

  ImmediateFuture<std::unique_ptr<InodeBase>> startLoadingInode(
      const DirEntry& entry,
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool async);

  /**
   * Invalidate old non-materialized children's recursively.
   *
   * File inodes touched before the passed in cutoff will be invalidated. Tree
   * inodes will also be invalidated if all of their children's have been
   * invalidated.
   *
   * Returns the number of tree inodes invalidated underneath this tree (for
   * logging purposes) and if this inode and all of its descendants were
   * invalidated (for use as an unloading parameter)
   */
  ImmediateFuture<std::pair<
      uint64_t /* numInvalidated */,
      bool /* allDescendantsInvalidated */>>
  invalidateChildrenNotMaterializedNFS(
      std::chrono::system_clock::time_point cutoff,
      const ObjectFetchContextPtr& context,
      folly::CancellationToken cancellationToken = {});

  ImmediateFuture<uint64_t /* numInvalidated */>
  invalidateChildrenNotMaterializedPrjFS(
      std::chrono::system_clock::time_point cutoff,
      const ObjectFetchContextPtr& context,
      folly::CancellationToken cancellationToken = {});

#ifndef _WIN32
  /**
   * Active FUSE invalidation for pressure-based GC.
   *
   * Walks the inode tree bottom-up, sending FUSE_NOTIFY_INVAL_ENTRY for each
   * stale entry. This causes the kernel to send FORGET for those inodes,
   * decrementing fsRefcount so they can be unloaded.
   *
   * Both materialized and non-materialized inodes are invalidated —
   * invalidateEntry only drops the dcache entry, overlay data is preserved.
   */
  ImmediateFuture<uint64_t /* numInvalidated */>
  invalidateChildrenNotAccessedRecentlyFuse(
      std::chrono::system_clock::time_point cutoff,
      const ObjectFetchContextPtr& context,
      const folly::CancellationToken& cancellationToken = {});

  ImmediateFuture<uint64_t /* numInvalidated */>
  invalidateChildrenNotAccessedRecentlyFuseImpl(
      std::chrono::system_clock::time_point cutoff,
      const ObjectFetchContextPtr& context,
      const folly::CancellationToken& cancellationToken,
      const std::shared_ptr<const GcBarrierTrie>& gcBarrier,
      const GcBarrierTrie* FOLLY_NULLABLE currentGcBarrier);
#endif

  /**
   * Materialize this directory in the overlay.
   *
   * This is required whenever we are about to make a structural change
   * in the tree; renames, creation, deletion.
   */
  void materialize(const RenameLock* renameLock = nullptr);

  [[nodiscard]] ImmediateFuture<folly::Unit> doRename(
      TreeRenameLocks&& locks,
      PathComponentPiece srcName,
      PathMap<DirEntry>::iterator srcIter,
      TreeInodePtr destParent,
      PathComponentPiece destName,
      InvalidationRequired invalidate);

  Overlay* getOverlay() const;

  /**
   * Loads a tree from the overlay given an inode number.
   */
  DirContents loadOverlayDir(InodeNumber inodeNumber) const;

  /**
   * Saves the entries of this inode to the overlay.
   */
  void saveOverlayDir(const DirContents& contents, bool isMaterialized = true)
      const;

  /**
   * Saves the entries for a specified inode number.
   */
  void saveOverlayDir(
      InodeNumber inodeNumber,
      const DirContents& contents,
      bool isMaterialized = true) const;

  /**
   * Converts a Tree to a Dir and saves it to the Overlay under the given inode
   * number.
   */
  static DirContents
  saveDirFromTree(InodeNumber inodeNumber, const Tree* tree, EdenMount* mount);

  /** Translates a Tree object from our store into a Dir object
   * used to track the directory in the inode */
  static DirContents buildDirFromTree(
      const Tree* tree,
      Overlay* overlay,
      CaseSensitivity caseSensitive);

  void updateAtime();

  void considerReaddirPrefetch(const ObjectFetchContextPtr& context);

  /**
   * Bitset indicating what entry types should be prefetched.
   */
  using PrefetchSet = uint8_t;
  enum : PrefetchSet {
    PrefetchFiles = 1,
    PrefetchTrees = 2,
  };

  void doPrefetch(PrefetchSet prefetchSet, const ObjectFetchContext& context);

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
   * Helper function to implement both fuseReaddir and nfsReaddir.
   *
   * Returns a boolean that indicates if readdir finished reading the entire
   * directory.
   */
  template <typename Fn>
  bool readdirImpl(off_t offset, const ObjectFetchContextPtr& context, Fn add);

 public:
  /**
   * Returns true if this directory is restricted by a path ACL.
   * Restricted directories deny access to their contents.
   */
  bool isRestricted() const {
    return aclRootState() == AclRootState::RestrictedAclRoot;
  }

  /**
   * Returns true if this directory is structurally covered by an ACL root.
   * Unlike isRestricted(), this is independent from the caller's access.
   */
  std::optional<bool> hasACL() const {
    return hasACLFromAclRootState(aclRootState());
  }

  AclRootState aclRootState() const {
    return static_cast<AclRootState>(
        aclRootState_.load(std::memory_order_relaxed));
  }

 private:
  void assertRestrictedPlaceholderInvariant() const;

  void setAclRootState(AclRootState state) {
    aclRootState_.store(static_cast<uint8_t>(state), std::memory_order_relaxed);
  }

  void setRestricted(bool isRestricted) {
    setAclRootState(makeAclRootState(isRestricted, hasACL()));
    if (isRestricted) {
      assertRestrictedPlaceholderInvariant();
      lastPermissionCheck_.store(
          std::chrono::steady_clock::time_point::min(),
          std::memory_order_relaxed);
    }
  }

  void setHasACL(std::optional<bool> hasACL) {
    setAclRootState(makeAclRootState(isRestricted(), hasACL));
  }

  /**
   * Throws EACCES if this directory is restricted by a path ACL.
   * Called by guarded lock accessors before contents_ acquisition.
   *
   * "has_acl" in the Sapling layer means a tree entry has an ACL file
   * present — it does not imply restriction. isRestricted is only set
   * after a permission check determines the user lacks access.
   */
  void checkAccess() const {
    if (FOLLY_UNLIKELY(isRestricted())) {
      throwRestrictedAccess();
    }
  }

  /**
   * Throws EACCES with inode context for restricted directory access.
   */
  [[noreturn]] void throwRestrictedAccess() const;

  /**
   * If this inode is restricted and the TTL has expired, re-validate
   * permissions via check_permission. If the user now has access,
   * calls transitionToUnrestricted() to populate this inode.
   */
  ImmediateFuture<folly::Unit> recheckPermissionIfExpired(
      const ObjectFetchContextPtr& fetchContext);

  /**
   * Transition this inode from restricted to unrestricted. Fetch the real
   * tree, rebuild DirContents, and install it under the usual rename/content
   * locks.
   */
  ImmediateFuture<folly::Unit> transitionToUnrestricted(
      const ObjectFetchContextPtr& fetchContext);

  /**
   * Build DirContents for an unrestricted tree by checking the overlay
   * first (preserving existing inode numbers), falling back to
   * saveDirFromTree() for fresh allocation. Reports whether stale denied
   * ACL root states were refreshed. Reused by startLoadingInode() and
   * transitionToUnrestricted().
   */
  struct BuildUnrestrictedDirContentsResult {
    DirContents contents;
    bool refreshedStaleDeniedAclRootStates{false};
  };

  BuildUnrestrictedDirContentsResult buildUnrestrictedDirContents(
      InodeNumber inodeNumber,
      const Tree& tree,
      std::optional<MiniTracer::Span> loadOverlayDirSpan = std::nullopt);

  /**
   * createImpl() is a helper function for creating new children inodes.
   *
   * This is used by create(), symlink(), and mknod().
   */
  FileInodePtr createImpl(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      PathComponentPiece name,
      mode_t mode,
      folly::ByteRange fileContents,
      InvalidationRequired invalidate,
      std::chrono::system_clock::time_point startTime);

  /**
   * removeImpl() is the actual implementation used for unlink() and rmdir().
   *
   * The child inode in question must already be loaded.  removeImpl() will
   * confirm that this is still the correct inode for the given name, and
   * remove it if so.  If not it will attempt to load the child again, and will
   * retry the remove again (hence the attemptNum parameter).
   */
  template <typename InodePtrType>
  [[nodiscard]] ImmediateFuture<folly::Unit> removeImpl(
      PathComponent name,
      InodePtr child,
      InvalidationRequired invalidate,
      unsigned int attemptNum,
      const ObjectFetchContextPtr& fetchContext);

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
  [[nodiscard]] int tryRemoveChild(
      const RenameLock& renameLock,
      PathComponentPiece name,
      InodePtrType child,
      InvalidationRequired invalidate);

  /**
   * checkPreRemove() is called by tryRemoveChild() for file or directory
   * specific checks before unlinking an entry.  Returns an errno value or 0.
   */
  [[nodiscard]] static int checkPreRemove(const TreeInode& child);
  [[nodiscard]] static int checkPreRemove(const FileInode& child);

  /**
   * This helper function starts loading a currently unloaded child inode.
   * Must be called with the contents_ lock held.
   */
  ImmediateFuture<InodePtr> loadChildLocked(
      PathComponentPiece name,
      DirEntry& entry,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      const ObjectFetchContextPtr& fetchContext);

  /**
   * Load the .gitignore file for this directory, then call computeDiff() once
   * it is loaded.
   */
  [[nodiscard]] ImmediateFuture<folly::Unit> loadGitIgnoreThenDiff(
      InodePtr gitignoreInode,
      DiffContext* context,
      RelativePathPiece currentPath,
      std::vector<std::shared_ptr<const Tree>> trees,
      const GitIgnoreStack* parentIgnore,
      bool isIgnored);

  folly::coro::now_task<folly::Unit> co_loadGitIgnoreThenDiff(
      InodePtr gitignoreInode,
      DiffContext* context,
      RelativePathPiece currentPath,
      std::vector<std::shared_ptr<const Tree>> trees,
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
  [[nodiscard]] ImmediateFuture<folly::Unit> computeDiff(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      DiffContext* context,
      RelativePathPiece currentPath,
      const std::vector<std::shared_ptr<const Tree>>& trees,
      std::unique_ptr<GitIgnoreStack> ignore,
      bool isIgnored);

  folly::coro::now_task<folly::Unit> co_computeDiff(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      DiffContext* context,
      RelativePathPiece currentPath,
      std::vector<std::shared_ptr<const Tree>> trees,
      std::unique_ptr<GitIgnoreStack> ignore,
      bool isIgnored);

  /**
   * Shared synchronous first pass of computeDiff / co_computeDiff.
   *
   * Walks the merge of `trees` and this directory's inode entries under
   * `contentsLock`, builds the DeferredDiffEntry list, releases the lock, and
   * finishes any pending inode loads started during the walk. The caller is
   * responsible for keeping `ignore` alive until the returned deferred entries
   * have completed.
   */
  std::vector<std::unique_ptr<DeferredDiffEntry>> prepareDeferredDiffEntries(
      folly::Synchronized<TreeInodeState>::LockedPtr contentsLock,
      DiffContext* context,
      RelativePathPiece currentPath,
      const std::vector<std::shared_ptr<const Tree>>& trees,
      GitIgnoreStack* ignore,
      bool isIgnored);

  /**
   * Check to see if we can break out of a checkout() operation early.
   *
   * This should only be called for non-materialized TreeInodes that have a
   * source control id.
   *
   * @param ctx The CheckoutContext
   * @param treeId The source control id for the TreeInode being updated.
   * @param fromTree The source control Tree that this checkout operation is
   *        moving away from.  This may be null if there was no source control
   *        state at this location previously.
   * @param toTree The destination source control Tree of the checkout.
   *        of the checkout).  This may be null if the destination state has no
   *        contents under this directory.
   */
  static bool canShortCircuitCheckout(
      CheckoutContext* ctx,
      const ObjectId& treeId,
      AclRootState aclRootState,
      const Tree* fromTree,
      const Tree* toTree);
  void computeCheckoutActions(
      CheckoutContext* ctx,
      const Tree* fromTree,
      const Tree* toTree,
      std::vector<std::shared_ptr<CheckoutAction>>& actions,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& wasDirectoryListModified,
      bool& hadConflicts,
      bool reportLocalOnlyAsConflicts);

  /**
   * Sets wasDirectoryListModified true if this checkout entry operation has
   * modified the directory contents, which implies the return value is
   * nullptr.
   *
   * This function could return a std::variant of InvalidationRequired and
   * std::shared_ptr<CheckoutAction> instead of setting a boolean.
   */
  template <typename Contents>
  std::shared_ptr<CheckoutAction> processCheckoutEntry(
      CheckoutContext* ctx,
      TreeInodeState& state,
      Contents& contents,
      const Tree::value_type* oldScmEntry,
      const Tree::value_type* newScmEntry,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& wasDirectoryListModified,
      bool& hadConflicts);

  std::shared_ptr<CheckoutAction> processLocalOnlyCheckoutEntry(
      CheckoutContext* ctx,
      DirContents::iterator it,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& hadConflicts);

  template <typename Contents>
  std::shared_ptr<CheckoutAction> processCheckoutEntryImpl(
      CheckoutContext* ctx,
      TreeInodeState& state,
      Contents& contents,
      const Tree::value_type* oldScmEntry,
      const Tree::value_type* newScmEntry,
      std::vector<IncompleteInodeLoad>& pendingLoads,
      bool& wasDirectoryListModified,
      bool& hadConflicts);

  template <typename Contents>
  folly::Try<folly::Unit> removeOrReplaceCheckoutEntryLocked(
      CheckoutContext* ctx,
      TreeInodeState& state,
      Contents& contents,
      typename Contents::iterator it,
      const InodePtr& loadedChild,
      const Tree::value_type* newScmEntry,
      bool& wasDirectoryListModified);

  template <typename Contents>
  std::shared_ptr<CheckoutAction> processAbsentCheckoutEntry(
      CheckoutContext* ctx,
      TreeInodeState& state,
      Contents& contents,
      const Tree::value_type* oldScmEntry,
      const Tree::value_type* newScmEntry,
      bool& wasDirectoryListModified,
      bool& hadConflicts);

  void saveOverlayPostCheckout(CheckoutContext* ctx, const Tree* tree);

  /**
   * Send a request to the kernel to invalidate its directory cache for this
   * inode.  This is required when the child entry list has changed.
   * invalidateChannelEntryCache(state, name) only works if the entry name is
   * known to the channel (FUSE, PrjFS), which is not true for new entries.
   *
   * A TreeInodeState is required as a way to ensure that contents_ lock is
   * being held to avoid races between invalidation during checkout and use
   * lookups.
   *
   * On NFS, we use the mode bits as part of invalidation. If this inode's
   * permission bits are updated, invalidateChannelDirCache must be called on
   * the parent inode afterwards.
   */
  [[nodiscard]] ImmediateFuture<folly::Unit> invalidateChannelDirCache(
      TreeInodeState&);

  /**
   * Coroutine version of `invalidateChannelDirCache`.
   *
   * Split into two halves so the FUSE/NFS sync work and Windows path
   * snapshot can be done under the caller's contents_ wlock, then the
   * lock released before the Windows PrjFS dispatch runs on the
   * invalidation thread pool.
   */
  struct InvalidationSnapshot {
#ifdef _WIN32
    PrjfsChannel* prjfsChannel{nullptr};
    std::optional<RelativePath> windowsPath;
#endif
  };

  /**
   * Synchronous half: runs FUSE/NFS invalidation under the caller's
   * contents_ wlock and returns a snapshot for the async tail.
   */
  [[nodiscard]] InvalidationSnapshot prepareInvalidateDirCache(TreeInodeState&);

  /**
   * Asynchronous half: runs the Windows PrjFS dispatch (no-op on
   * non-Windows). Caller must have released the contents_ wlock first.
   */
  [[nodiscard]] folly::coro::now_task<folly::Unit> co_finishInvalidateDirCache(
      InvalidationSnapshot snapshot);

  /**
   * Send a request to the kernel to invalidate its cache for the given child
   * entry name. On unices this corresponds to the dcache entry which caches
   * name lookups to child inodes. On Windows, this removes the on-disk
   * placeholder.
   *
   * This should be called when an entry is added, removed, or changed.
   * Invalidating upon removal is required because the kernel maintains a
   * negative cache on lookup failures on Unices.
   *
   * A TreeInodeState is required as a way to ensure that contents_ lock is
   * being held to avoid races between invalidation during checkout and use
   * lookups.
   */
  [[nodiscard]] folly::Try<folly::Unit> invalidateChannelEntryCache(
      TreeInodeState&,
      PathComponentPiece name,
      std::optional<InodeNumber> ino);

#ifndef _WIN32
  /**
   * Sends a request to the kernel to invalidate its cache for this tree and
   * then deletes all its children's inode. In NFS, this function is distinct
   * from `invalidateChannelEntryCache` and is used exclusively for garbage
   * collection because inodes need to be deleted after invalidation during NFS
   * garbage collection.
   */
  [[nodiscard]] folly::Try<folly::Unit> nfsInvalidateCacheEntryForGC(
      TreeInodeState& state);
#endif

  /**
   * Attempts to find the child inode or other identifying information about
   * the inode with out performing any write operations. `loadInodes` indicates
   * that you would like to load the inodes if they are not yet loaded. If the
   * inode is not loaded and `loadInodes` is set, a nullopt value will be
   * returned and you can call wlockGetOrFindChild to load and return the
   * inode.
   *
   * If the inode is already loaded this will return the inode.
   * Otherwise, if loadInodes is set or the inode is materialized we will
   * return nullopt because the inode must be loaded to inspect it and loading
   * an inode is a write operation. If we fall into none of the above cases the
   * TreeOrEntry representing the data for that inode will be returned.
   */
  std::optional<ImmediateFuture<VirtualInode>> rlockGetOrFindChild(
      const TreeInodeState& contents,
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool loadInodes);

  /**
   * Coroutine equivalent of rlockGetOrFindChild. Checks if a child entry can
   * be resolved without loading an inode. Returns a VirtualInode for
   * synchronous cases (loaded inode, unmaterialized file). Sets dirFetch for
   * unmaterialized directories that need an async tree fetch after the lock
   * is released. Returns nullopt if an inode load is needed (requires wlock).
   */
  struct PendingDirFetch {
    ObjectId treeId;
    mode_t mode{};
    std::optional<bool> hasACL{std::nullopt};
  };

  std::optional<VirtualInode> rlockCheckChild(
      const TreeInodeState& contents,
      PathComponentPiece name,
      const ObjectFetchContextPtr& context,
      bool loadInodes,
      std::optional<PendingDirFetch>& dirFetch);

  // We need to do some cleanup outside of the lock. So we return some promises
  // and futures and things to fulfil after the lock is released.
  struct LoadChildCleanUp {
    // If we are responsible for loading the inode, but the load is not
    // complete yet, then we need to register the inode load, so that someone
    // will take care of the cleanup after loading the inode. This future will
    // be valid if we are the ones responsible for the inode load.
    folly::Future<std::unique_ptr<InodeBase>> inodeLoadFuture;

    // If we are the ones responsible for the inode load and the load is
    // complete, then these are the promises we need to notify.
    std::vector<folly::Promise<InodePtr>> promises;

    // The inode number of the child we are getting.
    InodeNumber childNumber;

    // If we are the ones responsible for the load and the load already
    // completed here is the InodePointer.
    InodePtr childInodePtr;
  };

  /**
   * Loads and returns the inode for this child. Note this does not perform
   * inode load cleanup. loadChildCleanup must be called after the lock has
   * been released, any code between calling this and loadChildCleanUp should
   * be no throw or call loadChildCleanUp despite exceptions.
   */
  std::pair<folly::SemiFuture<InodePtr>, LoadChildCleanUp> loadChild(
      folly::Synchronized<TreeInodeState>::LockedPtr& contents,
      PathComponentPiece name,
      const ObjectFetchContextPtr& context);

  /**
   * Handles the inode loading related clean up for a wlockGetOrFindChild call.
   * This should be called without the contents lock held!
   */
  void loadChildCleanUp(PathComponentPiece name, LoadChildCleanUp result);

  folly::Synchronized<TreeInodeState> contents_;

  /**
   * Direct ACL root state for this directory. Restricted directories deny
   * access to their contents; permitted directories are ACL roots the caller
   * can read.
   */
  std::atomic<uint8_t> aclRootState_{
      static_cast<uint8_t>(AclRootState::Unknown)};

  /**
   * Timestamp of the last permission recheck attempt for restricted inodes.
   * Cached restricted inodes with unknown check provenance start at
   * time_point::min() to force one recheck after load; a denied recheck updates
   * this to now() and resumes TTL throttling.
   */
  std::atomic<std::chrono::steady_clock::time_point> lastPermissionCheck_{};

  /**
   * Valid state transitions:
   *   NeverEnumerated -> Enumerated
   *   Enumerated -> PrefetchedTrees
   *   Enumerated -> PrefetchedAll
   *   PrefetchedTrees -> PrefetchedAll
   */
  enum PrefetchState : uint8_t {
    /// readdir() has not been called on this inode since it was loaded.
    NeverEnumerated,
    /// readdir() has been called.
    Enumerated,
    /// A child tree was stat() and we have prefetched all child trees.
    PrefetchedTrees,
    /// A child file was stat() and we have prefetched inodes for all children.
    PrefetchedAll,
  };

  /**
   * Only prefetch children aux data once.
   */
  std::atomic<PrefetchState> prefetchState_{NeverEnumerated};

  /**
   * This number is not guaranteed to be completely accurate as it is modified
   * by the descendant using getParentRacy. It is currently only used for
   * reporting checkout progress to Sapling, so this is okay for that purpose.
   */
  std::atomic<int64_t> inMemoryDescendants_;
};

/**
 * An internal function which computes the difference between a Dir and a tree
 * as a set of strings starting with + and - followed by the entry name.
 */
std::optional<std::vector<std::string>> findEntryDifferences(
    const DirContents& dir,
    const Tree& tree);

} // namespace facebook::eden
