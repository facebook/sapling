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

#include <folly/Synchronized.h>
#include <folly/futures/Future.h>
#include <list>
#include <memory>
#include <unordered_map>

#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/gen-cpp2/handlemap_types.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class exception_wrapper;
}

namespace facebook {
namespace eden {

class EdenMount;
class FileInode;
class InodeBase;
class TreeInode;
class ParentInodeInfo;

struct UnloadedInodeData {
  UnloadedInodeData(fusell::InodeNumber p, PathComponentPiece n)
      : parent(p), name(n) {}

  fusell::InodeNumber const parent;
  PathComponent const name;
};

class InodeMapLock;

/**
 * InodeMap allows looking up Inode objects based on a inode number
 * (fusell::InodeNumber).
 *
 * All operations on this class are thread-safe.
 *
 * Note that fusell::InodeNumber values and Inode objects have separate
 * lifetimes:
 * - fusell::InodeNumber numbers are allocated when we need to return a
 * fusell::InodeNumber value to the FUSE APIs.  These are generally allocated by
 * lookup() calls. FUSE will call forget() to let us know when we can forget
 * about a fusell::InodeNumber value.  (We may not necessarily forget about the
 * fusell::InodeNumber value immediately, though.)
 *
 * - InodeBase objects are needed when we have to actually operate on a file or
 *   directory.  Any operation more than looking up a file name (or some of its
 *   basic attributes that can be found in its parent directory's entry for it)
 *   requires loading its Inode.
 *
 *   After we load an InodeBase object we keep it loaded in memory, since it is
 *   likely to be needed again in the future if the user keeps using the
 *   file/directory.  However, we can unload InodeBase objects on demand when
 *   they are not being used by other parts of the code.  This helps us reduce
 *   our memory footprint.  (For instance, if a user runs a command that walks
 *   the entire repository we don't want to keep Inode objects loaded for all
 *   files forever.)
 *
 * We can unload an InodeBase object even when the fusell::InodeNumber value is
 * still valid.  Therefore this class contains two separate maps:
 * - (fusell::InodeNumber --> InodeBase)
 *   This map stores all currently loaded Inodes.
 * - (fusell::InodeNumber --> (parent fusell::InodeNumber, name))
 *   This map contains information about Inodes that are not currently loaded.
 *   This map contains enough information to identify the file or directory in
 *   question if we do need to load the inode.  The parent directory's
 *   fusell::InodeNumber may not be loaded either; we have to load it first in
 * order to load the inode in question.
 *
 * Additional Notes about fusell::InodeNumber allocation:
 * - fusell::InodeNumber numbers are primarily allocated via the
 *   EdenDispatcher::lookup() call.
 *
 *   Rather than only allocate a fusell::InodeNumber in this case, we go ahead
 * and load the actual TreeInode/FileInode, since FUSE is very likely to make
 * another call for this inode next.  Therefore, in this case the newly
 * allocated inode number is inserted directly into loadedInodes_, without ever
 * being in unloadedInodes_.
 *
 *   The unloadedInodes_ map is primarily for inodes that were loaded and have
 *   since been unloaded due to inactivity.
 *
 * - Inode objects can either be allocated via EdenDispatcher::lookup(), or via
 *   an operation on a TreeInode looking up a child entry.
 *
 *   We currently always allocate a fusell::InodeNumber value for any new Inode
 * object even if it is not needed yet by the FUSE APIs.
 */
class InodeMap {
 public:
  using PromiseVector = std::vector<folly::Promise<InodePtr>>;

  explicit InodeMap(EdenMount* mount);
  virtual ~InodeMap();

  InodeMap(InodeMap&&) = default;
  InodeMap& operator=(InodeMap&&) = default;

  /**
   * Initialize the InodeMap
   *
   * This method must be called shortly after constructing an InodeMap object,
   * before it is visible to other threads.  This method is not thread safe.
   *
   * This is provided as a separate method from the constructor purely to
   * provide callers with slightly more flexibility in ordering of events when
   * constructing an InodeMap.  This generally should be thought of as part of
   * the InodeMap construction process, though.
   *
   * @param root The root TreeInode.
   * @param maxExistingInode  The maximum inode number currently assigned to
   *     any inode in the filesystem.  For newly created file systems this
   *     should be FUSE_ROOT_ID.  If this is a mount point that has been
   *     mounted before, this should be the maximum value across all the
   *     outstanding inodes.
   */
  void initialize(TreeInodePtr root, fusell::InodeNumber maxExistingInode);

  /**
   * Get the root inode.
   */
  const TreeInodePtr& getRootInode() const {
    return root_;
  }

  /**
   * Lookup an Inode object by inode number.
   *
   * Inode objects can only be looked up by number if the inode number
   * reference count is non-zero.  The inode number refcount is incremented by
   * calling incFuseRefcount() on the Inode object.  The initial access that
   * first creates an Inode is always by name.  After the initial access,
   * incFuseRefcount() can be called to allow it to be retreived by inode
   * number later.  InodeMap::decFuseRefcount() can be used to drop an inode
   * number reference count.
   *
   * If the InodeBase object is not currently loaded it will be loaded and a
   * new InodeBase object returned.
   *
   * Loading an Inode requires retreiving data about it from the ObjectStore,
   * which may take some time.  Therefore lookupInode() returns a Future, which
   * will be fulfilled when the loaded Inode is ready.  The Future may be
   * invoked immediately in the calling thread (if the Inode is already
   * available), or it may be invoked later in a different thread.
   */
  folly::Future<InodePtr> lookupInode(fusell::InodeNumber number);

  /**
   * Lookup a TreeInode object by inode number.
   *
   * This creates the TreeInode object if it is not currently loaded.
   * The returned Future throws ENOTDIR if this inode number does not refer to
   * a directory.
   */
  folly::Future<TreeInodePtr> lookupTreeInode(fusell::InodeNumber number);

  /**
   * Lookup a FileInode object by inode number.
   *
   * This creates the FileInode object if it is not currently loaded.
   * The returned Future throws EISDIR if this inode number refers to a
   * directory.
   */
  folly::Future<FileInodePtr> lookupFileInode(fusell::InodeNumber number);

  /**
   * Lookup an Inode object by inode number, if it alread exists.
   *
   * This returns an existing InodeBase object if one is currently loaded,
   * but nullptr if one is not loaded.  lookupUnloadedInode() can then be
   * called to find data for the unloaded inode.
   */
  InodePtr lookupLoadedInode(fusell::InodeNumber number);

  /**
   * Lookup a loaded TreeInode object by inode number, if it alread exists.
   *
   * Returns nullptr if this inode object is not currently loaded.
   * Throws ENOTDIR if this inode is loaded but does not refer to a directory.
   */
  TreeInodePtr lookupLoadedTree(fusell::InodeNumber number);

  /**
   * Lookup a loaded FileInode object by inode number, if it alread exists.
   *
   * Returns nullptr if this inode object is not currently loaded.
   * Throws EISDIR if this inode is loaded but refers to a directory.
   */
  FileInodePtr lookupLoadedFile(fusell::InodeNumber number);

  /**
   * Lookup data about an unloaded Inode object.
   *
   * Callers should generally call lookupLoadedInode() first to make sure the
   * inode is not currently loaded.  Unloaded inode data will only be found if
   * the object is not currently loaded.
   */
  UnloadedInodeData lookupUnloadedInode(fusell::InodeNumber number);

  /**
   * Recursively determines the path for a loaded or unloaded inode. If the
   * inode is unloaded, it appends the name of the unloaded inode to the path
   * of the parent inode (which is determined recursively). If the inode is
   * loaded, it returns InodeBase::getPath() (which also recursively
   * queries the parent nodes).
   *
   * If there is an unlinked inode in the path, this function returns
   * folly::none. If the inode is invalid, it throws EINVAL.
   */
  folly::Optional<RelativePath> getPathForInode(
      fusell::InodeNumber inodeNumber);

  /**
   * Decrement the number of outstanding FUSE references to an inode number.
   *
   * Note that there is no corresponding incFuseRefcount() function:
   * increments are always done directly on a loaded InodeBase object.
   *
   * However, decrements may happen after we have decided to unload the Inode
   * object.  Therefore decrements are performed on the InodeMap so that we can
   * avoid loading an Inode object just to decrement its reference count.
   */
  void decFuseRefcount(fusell::InodeNumber number, uint32_t count = 1);

  /**
   * Persist the inode number state to an instance of InodeMap::TakeoverData.
   *
   * This API supports gracefully restarting the eden server without unmounting
   * the mount point.
   *
   * This persists sufficient data to reconstruct all inode state into the
   * unloadedInodes_ map.
   */
  SerializedInodeMap save();
  void load(const SerializedInodeMap& takeover);

  /**
   * Shutdown the InodeMap.
   *
   * The shutdown process must wait for all Inode objects in this InodeMap to
   * become unreferenced and be unloaded.  Returns a Future that will be
   * fulfilled once the shutdown has completed.
   *
   * This function should generally only be invoked by EdenMount::shutdown().
   * Other callers should use EdenMount::shutdown() instead of invoking this
   * function directly.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> shutdown();

  /**
   * onInodeUnreferenced() will be called when an Inode's InodePtr reference
   * count drops to zero.
   *
   * This is an internal API that should not be called by most users.
   */
  void onInodeUnreferenced(
      const InodeBase* inode,
      ParentInodeInfo&& parentInfo);

  /**
   * Acquire the InodeMap lock while performing Inode unloading.
   *
   * This method is called by TreeInode when scanning its children for
   * unloading.  It should only be called *after* acquring the TreeInode
   * contents lock.
   *
   * This is an internal API that should not be used by most callers.
   */
  InodeMapLock lockForUnload();

  /**
   * unloadedInode() should be called to unload an unreferenced inode.
   *
   * The caller is responsible for ensuring that this inode is unreferenced and
   * safe to unload.  The caller must have previously locked the parent
   * TreeInode's contents map  (unless the inode is unlinked, in which case it
   * no longer has a parent).  The caller must have also locked the InodeMap
   * using lockForUnload().
   *
   * This is an internal API that should not be used by most callers.
   *
   * The caller is still responsible for deleting the InodeBase.
   * The InodeBase should not be deleted until releasing the InodeMap and
   * TreeInode locks (since deleting it may cause its parent Inode to become
   * unreferenced, triggering another immediate call to onInodeUnreferenced(),
   * which will acquire these locks).
   */
  void unloadInode(
      const InodeBase* inode,
      TreeInode* parent,
      PathComponentPiece name,
      bool isUnlinked,
      const InodeMapLock& lock);

  /////////////////////////////////////////////////////////////////////////
  // The following public APIs should only be used by TreeInode
  /////////////////////////////////////////////////////////////////////////

  /**
   * shouldLoadChild() should only be called by TreeInode.
   *
   * shouldLoadChild() will be called when TreeInode wants to load one of
   * its child entries that already has an allocated inode number.  It returns
   * true if the TreeInode should start loading the inode now, or false if the
   * inode is already being loaded.
   *
   * If shouldLoadChild() returns true, the TreeInode will then start
   * loading the child inode.  It must then call inodeLoadComplete() or
   * inodeLoadFailed() when it finishes loading the inode.
   *
   * The TreeInode must be holding its contents lock when calling this method.
   *
   * @param parent The TreeInode calling this function to check if it should
   *   load a child.
   * @param name The name of the child inode.
   * @param childInode The inode number of the child.
   * @param promise A promise to fulfill when this inode is finished loading.
   *   The InodeMap is responsible for fulfilling this promise.
   *
   * @return Returns true if the TreeInode should start loading this child
   *   inode, or false if this child is already being loaded.
   */
  bool shouldLoadChild(
      const TreeInode* parent,
      PathComponentPiece name,
      fusell::InodeNumber childInode,
      folly::Promise<InodePtr> promise);

  /**
   * newChildLoadStarted() should only be called by TreeInode.
   *
   * This will be called by TreeInode when it wants to start loading a new
   * child entry that does not already have an inode number allocated.
   *
   * Because no inode is allocated the TreeInode knows that no existing load
   * attempt is in progress.  If an inode number were allocated TreeInode would
   * need to call shouldLoadChild() instead.
   *
   * This function allocate an inode number for the child, record the pending
   * load operation, and then return the allocated inode number.
   *
   * The TreeInode must later call inodeLoadComplete() or inodeLoadFailed()
   * when it finishes loading the inode.
   *
   * The TreeInode must be holding its contents lock when calling this method.
   *
   * @param parent The TreeInode calling this function to check if it should
   *   load a child.
   * @param name The name of the child inode.
   * @param promise A promise to fulfil when this inode is finished loading.
   *   The InodeMap is responsible for fulfilling this promise.
   *
   * @return Returns the newly allocated child inode number.
   */
  fusell::InodeNumber newChildLoadStarted(
      const TreeInode* parent,
      PathComponentPiece name,
      folly::Promise<InodePtr> promise);

  /**
   * inodeLoadComplete() should only be called by TreeInode.
   *
   * The TreeInode must be holding its contents lock when calling this method.
   *
   * We update both the parent's contents map and the InodeMap while holding
   * the contents lock.  This ensures that if you lock a TreeInode and see that
   * an inode isn't present in its contents, it cannot have finished loading
   * yet in the InodeMap.
   *
   * Returns a vector of Promises waiting on this TreeInode to be loaded.  The
   * TreeInode must fulfill these promises after releasing its contents lock.
   */
  PromiseVector inodeLoadComplete(InodeBase* inode);

  /**
   * inodeLoadFailed() should only be called by TreeInode.
   *
   * This should be called when an attempt to load a child inode fails.
   */
  void inodeLoadFailed(
      fusell::InodeNumber number,
      const folly::exception_wrapper& exception);

  /**
   * allocateInodeNumber() should only be called by TreeInode.
   *
   * This can be called:
   * - To allocate an inode number for an existing tree entry that does not
   *   need to be loaded yet.
   * - To allocate an inode number for a brand new inode being created by
   *   TreeInode::create() or TreeInode::mkdir().  In this case
   *   inodeCreated() should be called immediately afterwards to register the
   *   new child Inode object.
   */
  fusell::InodeNumber allocateInodeNumber();
  void inodeCreated(const InodePtr& inode);

  uint64_t getLoadedInodeCount() {
    return data_.rlock()->loadedInodes_.size();
  }
  uint64_t getUnloadedInodeCount() {
    return data_.rlock()->unloadedInodes_.size();
  }

 private:
  friend class InodeMapLock;

  /**
   * Data about an unloaded inode.
   *
   * Note that this is different from the public UnloadedInodeData type which
   * we return to callers.  This class tracks more state.
   */
  struct UnloadedInode {
    UnloadedInode(
        fusell::InodeNumber num,
        fusell::InodeNumber parentNum,
        PathComponentPiece entryName)
        : number(num), parent(parentNum), name(entryName) {}

    fusell::InodeNumber const number;
    fusell::InodeNumber const parent;
    PathComponent const name;

    /**
     * A boolean indicating if this inode is unlinked.
     *
     * TODO: For unlinked inodes we can't rely on the parent TreeInode to have
     * data about this inode's mode and File/Blob hash.  We need to record
     * enough information to be able to load it without a parent Tree.  We
     * perhaps should record this data in the overlay and just use the overlay.
     */
    bool isUnlinked{false};

    /** The complete st_mode value for this entry */
    mode_t mode{0};

    /**
     * If the entry is not materialized, this contains the hash
     * identifying the source control Tree (if this is a directory) or Blob
     * (if this is a file) that contains the entry contents.
     *
     * If the entry is materialized, this field is not set.
     */
    folly::Optional<Hash> hash;

    /**
     * A list of promises waiting on this inode to be loaded.
     *
     * If this list is non-empty then the inode is currently in the process of
     * being loaded.
     *
     * (We could use folly::SharedPromise here instead, but it has extra
     * overhead that we don't really need.  It performs its own locking, but we
     * are already protected by the data_ lock.)
     */
    PromiseVector promises;
    /**
     * The number of times we have returned this inode number to FUSE via
     * lookup() calls that have not yet been released with a corresponding
     * forget().
     */
    int64_t numFuseReferences{0};
  };

  struct Members {
    /**
     * The map of loaded inodes
     *
     * This map stores raw pointers rather than InodePtr objects.  The InodeMap
     * itself does not hold a reference to the Inode objects.  When an Inode is
     * looked up the InodeMap will wrap the Inode in an InodePtr so that the
     * caller acquires a reference.
     */
    std::unordered_map<fusell::InodeNumber, InodeBase*> loadedInodes_;

    /**
     * The map of currently unloaded inodes
     */
    std::unordered_map<fusell::InodeNumber, UnloadedInode> unloadedInodes_;

    /** The next inode number to allocate */
    fusell::InodeNumber nextInodeNumber_{FUSE_ROOT_ID + 1};

    /**
     * A promise to fulfill once shutdown() completes.
     *
     * This is only initialized when shutdown() is called, and will be
     * folly::none until we are shutting down.
     *
     * In the future we could update this to just use an empty promise to
     * indicate that we are not shutting down yet.  However, currently
     * folly::Promise does not have a simple API to check if it is empty or not,
     * so we have to wrap it in a folly::Optional.
     */
    folly::Optional<folly::Promise<folly::Unit>> shutdownPromise;
  };

  InodeMap(InodeMap const&) = delete;
  InodeMap& operator=(InodeMap const&) = delete;

  void shutdownComplete(folly::Synchronized<Members>::LockedPtr&& data);

  void setupParentLookupPromise(
      folly::Promise<InodePtr>& promise,
      PathComponentPiece childName,
      bool isUnlinked,
      fusell::InodeNumber childInodeNumber,
      folly::Optional<Hash> hash,
      mode_t mode);
  void startChildLookup(
      const InodePtr& parent,
      PathComponentPiece childName,
      bool isUnlinked,
      fusell::InodeNumber childInodeNumber,
      folly::Optional<Hash> hash,
      mode_t mode);

  /**
   * Extract the list of promises waiting on the specified inode number to be
   * loaded.
   *
   * This method acquires the data_ lock internally.
   * It should never be called while already holding the lock.
   */
  PromiseVector extractPendingPromises(fusell::InodeNumber number);

  fusell::InodeNumber allocateInodeNumber(Members& data);

  folly::Optional<RelativePath> getPathForInodeHelper(
      fusell::InodeNumber inodeNumber,
      const folly::Synchronized<Members>::ConstLockedPtr& data);

  /**
   * Unload an inode
   *
   * This simply removes it from the loadedInodes_ map and, if it is still
   * referenced by FUSE, adds it to the unloadedInodes_ map.
   *
   * The caller is responsible for actually deleting the Inode object after
   * releasing the InodeMap lock.
   */
  void unloadInode(
      const InodeBase* inode,
      TreeInode* parent,
      PathComponentPiece name,
      bool isUnlinked,
      const folly::Synchronized<Members>::LockedPtr& lock);

  /**
   * The EdenMount that owns this InodeMap.
   */
  EdenMount* const mount_{nullptr};

  /**
   * The root inode.
   *
   * This member never changes after the InodeMap is initialized.
   * It is therefore safe to access without locking.
   */
  TreeInodePtr root_;

  /**
   * The locked data.
   *
   * Note: be very careful to hold this lock only when necessary.  No other
   * locks should be acquired when holding this lock.  In particular this means
   * that we should never access any InodeBase objects while holding the lock,
   * since we should not hold our lock while an InodeBase acquires its own
   * internal lock.  (This makes it safe for InodeBase to perform operations on
   * the InodeMap while holding their own lock.)
   */
  folly::Synchronized<Members> data_;
};

/**
 * An opaque class so that InodeMap can return its lock to the TreeInode
 * in order to make multiple calls to unloadInode() without releasing and
 * re-acquiring the lock.
 *
 * This mostly exists to make forward declarations simpler.
 */
class InodeMapLock {
 public:
  explicit InodeMapLock(
      folly::Synchronized<InodeMap::Members>::LockedPtr&& data)
      : data_(std::move(data)) {}

  void unlock() {
    data_.unlock();
  }

 private:
  friend class InodeMap;
  folly::Synchronized<InodeMap::Members>::LockedPtr data_;
};
} // namespace eden
} // namespace facebook
