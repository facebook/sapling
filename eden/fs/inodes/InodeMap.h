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

#include "eden/fs/inodes/InodePtr.h"
#include "eden/fuse/fuse_headers.h"
#include "eden/utils/PathFuncs.h"

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
  UnloadedInodeData(fuse_ino_t p, PathComponentPiece n) : parent(p), name(n) {}

  fuse_ino_t const parent;
  PathComponent const name;
};

class InodeMapLock;

/**
 * InodeMap allows looking up Inode objects based on a inode number
 * (fuse_ino_t).
 *
 * All operations on this class are thread-safe.
 *
 * Note that fuse_ino_t values and Inode objects have separate lifetimes:
 * - fuse_ino_t numbers are allocated when we need to return a fuse_ino_t value
 *   to the FUSE APIs.  These are generally allocated by lookup() calls.
 *   FUSE will call forget() to let us know when we can forget about a
 *   fuse_ino_t value.  (We may not necessarily forget about the fuse_ino_t
 *   value immediately, though.)
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
 * We can unload an InodeBase object even when the fuse_ino_t value is still
 * valid.  Therefore this class contains two separate maps:
 * - (fuse_ino_t --> InodeBase)
 *   This map stores all currently loaded Inodes.
 * - (fuse_ino_t --> (parent fuse_ino_t, name))
 *   This map contains information about Inodes that are not currently loaded.
 *   This map contains enough information to identify the file or directory in
 *   question if we do need to load the inode.  The parent directory's
 *   fuse_ino_t may not be loaded either; we have to load it first in order to
 *   load the inode in question.
 *
 * Additional Notes about fuse_ino_t allocation:
 * - fuse_ino_t numbers are primarily allocated via the
 *   EdenDispatcher::lookup() call.
 *
 *   Rather than only allocate a fuse_ino_t in this case, we go ahead and load
 *   the actual TreeInode/FileInode, since FUSE is very likely to make another
 *   call for this inode next.  Therefore, in this case the newly allocated
 *   inode number is inserted directly into loadedInodes_, without ever being
 *   in unloadedInodes_.
 *
 *   The unloadedInodes_ map is primarily for inodes that were loaded and have
 *   since been unloaded due to inactivity.
 *
 * - Inode objects can either be allocated via EdenDispatcher::lookup(), or via
 *   an operation on a TreeInode looking up a child entry.
 *
 *   We currently always allocate a fuse_ino_t value for any new Inode object
 *   even if it is not needed yet by the FUSE APIs.
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
  void initialize(TreeInodePtr root, fuse_ino_t maxExistingInode);

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
  folly::Future<InodePtr> lookupInode(fuse_ino_t number);

  /**
   * Lookup a TreeInode object by inode number.
   *
   * This creates the TreeInode object if it is not currently loaded.
   * The returned Future throws ENOTDIR if this inode number does not refer to
   * a directory.
   */
  folly::Future<TreeInodePtr> lookupTreeInode(fuse_ino_t number);

  /**
   * Lookup a FileInode object by inode number.
   *
   * This creates the FileInode object if it is not currently loaded.
   * The returned Future throws EISDIR if this inode number refers to a
   * directory.
   */
  folly::Future<FileInodePtr> lookupFileInode(fuse_ino_t number);

  /**
   * Lookup an Inode object by inode number, if it alread exists.
   *
   * This returns an existing InodeBase object if one is currently loaded,
   * but nullptr if one is not loaded.  lookupUnloadedInode() can then be
   * called to find data for the unloaded inode.
   */
  InodePtr lookupLoadedInode(fuse_ino_t number);

  /**
   * Lookup a loaded TreeInode object by inode number, if it alread exists.
   *
   * Returns nullptr if this inode object is not currently loaded.
   * Throws ENOTDIR if this inode is loaded but does not refer to a directory.
   */
  TreeInodePtr lookupLoadedTree(fuse_ino_t number);

  /**
   * Lookup a loaded FileInode object by inode number, if it alread exists.
   *
   * Returns nullptr if this inode object is not currently loaded.
   * Throws EISDIR if this inode is loaded but refers to a directory.
   */
  FileInodePtr lookupLoadedFile(fuse_ino_t number);

  /**
   * Lookup data about an unloaded Inode object.
   *
   * Callers should generally call lookupLoadedInode() first to make sure the
   * inode is not currently loaded.  Unloaded inode data will only be found if
   * the object is not currently loaded.
   */
  UnloadedInodeData lookupUnloadedInode(fuse_ino_t number);

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
  void decFuseRefcount(fuse_ino_t number, uint32_t count = 1);

  /**
   * Persist the inode number state to disk.
   *
   * This API supports gracefully restarting the eden server without unmounting
   * the mount point.
   *
   * This persists sufficient data to reconstruct all inode state into the
   * unloadedInodes_ map.
   */
  void save();

  /**
   * beginShutdown() is invoked by EdenMount::destroy()
   *
   * The EdenMount can only be destroyed once all Inodes it contains are
   * unreferenced.  beginShutdown() initiates this process.  Once all Inodes
   * are destroyed the InodeMap will then delete the EdenMount.
   *
   * beginShutdown() should only be called by EdenMount.
   */
  void beginShutdown();

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
      TreeInode* parent,
      PathComponentPiece name,
      fuse_ino_t childInode,
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
  fuse_ino_t newChildLoadStarted(
      TreeInode* parent,
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
      fuse_ino_t number,
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
  fuse_ino_t allocateInodeNumber();
  void inodeCreated(const InodePtr& inode);

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
        fuse_ino_t num,
        fuse_ino_t parentNum,
        PathComponentPiece entryName)
        : number(num), parent(parentNum), name(entryName) {}

    fuse_ino_t const number;
    fuse_ino_t const parent;
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
    std::unordered_map<fuse_ino_t, InodeBase*> loadedInodes_;

    /**
     * The map of currently unloaded inodes
     */
    std::unordered_map<fuse_ino_t, UnloadedInode> unloadedInodes_;

    /** The next inode number to allocate */
    fuse_ino_t nextInodeNumber_{FUSE_ROOT_ID + 1};
  };

  InodeMap(InodeMap const&) = delete;
  InodeMap& operator=(InodeMap const&) = delete;

  void shutdownComplete();

  void setupParentLookupPromise(
      folly::Promise<InodePtr>& promise,
      PathComponentPiece childName,
      bool isUnlinked,
      fuse_ino_t childInodeNumber);
  void startChildLookup(
      const InodePtr& parent,
      PathComponentPiece childName,
      bool isUnlinked,
      fuse_ino_t childInodeNumber);

  /**
   * Extract the list of promises waiting on the specified inode number to be
   * loaded.
   *
   * This method acquires the data_ lock internally.
   * It should never be called while already holding the lock.
   */
  PromiseVector extractPendingPromises(fuse_ino_t number);

  fuse_ino_t allocateInodeNumber(Members& data);

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
   * shuttingDown_ will be set to true once EdenMount::destroy() is called
   */
  std::atomic<bool> shuttingDown_{false};

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
}
}
