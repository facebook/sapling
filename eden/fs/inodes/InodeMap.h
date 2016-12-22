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

class FileInode;
class InodeBase;
class TreeInode;

struct UnloadedInodeData {
  UnloadedInodeData(fuse_ino_t p, PathComponentPiece n) : parent(p), name(n) {}

  fuse_ino_t const parent;
  PathComponent const name;
};

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
  InodeMap();
  virtual ~InodeMap();

  InodeMap(InodeMap&&) = default;
  InodeMap& operator=(InodeMap&&) = default;

  /**
   * Set the root inode.
   *
   * This method must be called shortly after constructing an InodeMap object,
   * before it is visible to other threads.  This method is not thread safe.
   *
   * This is provided as a separate method from the constructor purely to
   * provide callers with slightly more flexibility in ordering of events when
   * constructing an InodeMap.  This generally should be thought of as part of
   * the InodeMap construction process, though.
   */
  void setRootInode(TreeInodePtr root);

  /**
   * Get the root inode.
   */
  const TreeInodePtr& getRootInode() const {
    return root_;
  }

  /**
   * Lookup an Inode object by inode number.
   *
   * This creates the InodeBase object if it is not currently loaded.
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
   * Persist the inode number state to disk.
   *
   * This API supports gracefully restarting the eden server without unmounting
   * the mount point.
   *
   * This persists sufficient data to reconstruct all inode state into the
   * unloadedInodes_ map.
   */
  void save();

  /////////////////////////////////////////////////////////////////////////
  // The following public APIs should only be used by TreeInode
  /////////////////////////////////////////////////////////////////////////

  /**
   * shouldLoadChild() should only be called by TreeInode.
   *
   * shouldLoadChild() will be called when TreeInode wants to load one of
   * its child entries by name.  It returns true if the TreeInode should start
   * loading the inode now, or false if the inode is already being loaded.
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
   * @param promise A promise to fulfil when this inode is finished loading.
   *   The InodeMap is responsible for fulfilling this promise.
   * @param childInodeReturn On return this will be set to the inode number of
   *   the specified child inode.  This return value is always populated,
   *   regardless of whether shouldLoadChild() returns true or false.
   *
   * @return Returns true if the TreeInode should start loading this child
   *   inode, or false if this child is already being loaded.
   */
  bool shouldLoadChild(
      TreeInode* parent,
      PathComponentPiece name,
      folly::Promise<InodePtr> promise,
      fuse_ino_t* childInodeReturn);

  /**
   * inodeLoadComplete() should only be called by TreeInode.
   *
   * The TreeInode must be holding its contents lock when calling this method.
   *
   * We update both the parent's contents map and the InodeMap while holding
   * the contents lock.  This ensures that if you lock a TreeInode and see that
   * an inode isn't present in its contents, it cannot have finished loading
   * yet in the InodeMap.
   */
  void inodeLoadComplete(const InodePtr& inode);

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
   * This should be called to allocate an inode number for a brand new inode
   * created by TreeInode::create() or TreeInode::mkdir()
   *
   * inodeCreated() must be called immediately afterwards to register the new
   * child Inode object.
   */
  fuse_ino_t allocateInodeNumber();
  void inodeCreated(const InodePtr& inode);

  /**
   * getOrAllocateUnloadedInodeNumber() should only be called by TreeInode.
   *
   * This method gets the inode number for an unloaded inode.  If an inode is
   * already assigned to this child that is returned.  Otherwise a new inode
   * number is allocated and assigned to the child, then returned.
   *
   * This should be called in situations where the inode number is needed by
   * the child Inode object does not actually need to be loaded yet.
   * The caller is responsible for guaranteeing that the child in question is
   * not currently loaded.
   *
   * The TreeInode must be holding its contents lock when calling this method.
   * Otherwise this method could race with an attempt to load the child.
   */
  fuse_ino_t getOrAllocateUnloadedInodeNumber(
      const TreeInode* parent,
      PathComponentPiece name);

 private:
  using PromiseVector = std::vector<folly::Promise<InodePtr>>;

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
    /*
     * TODO: We probably also need a reference count tracking how many times
     * this fuse_ino_t value has been returned to the FUSE API, so we know
     * when it is safe to forget.
     */
  };
  struct Members;

  InodeMap(InodeMap const&) = delete;
  InodeMap& operator=(InodeMap const&) = delete;

  void setupParentLookupPromise(
      folly::Promise<InodePtr>& promise,
      PathComponentPiece childName,
      fuse_ino_t childInodeNumber);
  void startChildLookup(
      const InodePtr& parent,
      PathComponentPiece childName,
      fuse_ino_t childInodeNumber);

  /**
   * Extract the list of promises waiting on the specified inode number to be
   * loaded.
   *
   * This method acquires the data_ lock internally.
   * It should never be called while already holding the lock.
   */
  PromiseVector extractPendingPromises(fuse_ino_t number);

  UnloadedInode* allocateUnloadedInode(
      Members& data,
      const TreeInode* parent,
      PathComponentPiece name);
  fuse_ino_t allocateInodeNumber(Members& data);

  struct Members {
    /**
     * The map of loaded inodes
     *
     * TODO: When we switch to our own custom InodePtr implementation,
     * this map should eventually store raw pointers, and not hold a reference
     * to the inodes.  The InodeMap itself should not force Inode objects to
     * remain in existence forever.
     */
    std::unordered_map<fuse_ino_t, InodePtr> loadedInodes_;

    /**
     * The map of currently unloaded inodes
     *
     * This stores the values as unique_ptr<UnloadedInode> rather than just a
     * plain UnloadedInode so that unloadedInodesReverse_ can store a
     * PathComponentPiece pointing to the values in this map.  This would not
     * work if the UnloadedInode objects could be moved.
     */
    std::unordered_map<fuse_ino_t, std::unique_ptr<UnloadedInode>>
        unloadedInodes_;

    /**
     * A reverse map of the unloaded inode data, allowing us to look up
     * UnloadedInode objects by (parent_number, name)
     *
     * This is needed to tell if we already have an inode number allocated when
     * doing a lookup by name.
     *
     * The UnloadedInode pointers in this map point to the values owned by the
     * unloadedInodes_ map.
     *
     * Note: Memory management for the key is slightly subtle.
     * The keys for this map contain PathComponentPieces instead of
     * PathComponents.  This allows lookup with a simple PathComponentPiece.
     * The PathComponentPiece in the key points to the PathComponent owned by
     * the UnloadedInode object.
     *
     * TODO: In the long run, we may need to just move all of this data into
     * the normal unloadedInodes_ map.  Each UnloadedInode should have a map
     * listing all its children that have fuse_ino_t values allocated but are
     * currently unloaded.  This will probably be necessary for TreeInode to
     * know which of its children are materialized but unloaded.
     */
    std::
        unordered_map<std::pair<fuse_ino_t, PathComponentPiece>, UnloadedInode*>
            unloadedInodesReverse_;

    /** The next inode number to allocate */
    fuse_ino_t nextInodeNumber_{FUSE_ROOT_ID + 1};
  };

  /**
   * The root inode.
   *
   * This member should never change after the InodeMap is initialized.
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
}
}
