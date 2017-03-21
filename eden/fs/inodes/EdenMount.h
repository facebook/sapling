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

#include <folly/SharedMutex.h>
#include <folly/Synchronized.h>
#include <memory>
#include <mutex>
#include <shared_mutex>
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/journal/JournalDelta.h"
#include "eden/utils/PathFuncs.h"

namespace folly {
template <typename T>
class Future;
}

namespace facebook {
namespace eden {
namespace fusell {
class Channel;
class MountPoint;
}

class BindMount;
class CheckoutConflict;
class ClientConfig;
class Dirstate;
class EdenDispatcher;
class InodeDiffCallback;
class InodeMap;
class ObjectStore;
class Overlay;
class Journal;
class Tree;

class RenameLock;
class SharedRenameLock;

/**
 * EdenMount contains all of the data about a specific eden mount point.
 *
 * This contains:
 * - The fusell::MountPoint object which manages our FUSE interactions with the
 *   kernel.
 * - The ObjectStore object used for retreiving/storing object data.
 * - The Overlay object used for storing local changes (that have not been
 *   committed/snapshotted yet).
 */
class EdenMount {
 public:
  EdenMount(
      std::unique_ptr<ClientConfig> config,
      std::unique_ptr<ObjectStore> objectStore);

  /**
   * Create a shared_ptr to an EdenMount.
   *
   * This is a convenience helper function to create the shared_ptr using an
   * EdenMountDeleter.
   */
  static std::shared_ptr<EdenMount> makeShared(
      std::unique_ptr<ClientConfig> config,
      std::unique_ptr<ObjectStore> objectStore);

  /**
   * Destroy the EdenMount.
   *
   * This begins the destruction process for the EdenMount.  The mount will
   * wait until all outstanding inode references are released before it is
   * completely destroyed.  (This may or may not happen before destroy()
   * returns.)
   */
  void destroy();

  /**
   * Get the MountPoint object.
   *
   * This returns a raw pointer since the EdenMount owns the mount point.
   * The caller should generally maintain a reference to the EdenMount object,
   * and not directly to the MountPoint object itself.
   */
  fusell::MountPoint* getMountPoint() const {
    return mountPoint_.get();
  }

  /**
   * Get the FUSE channel for this mount point.
   *
   * This should only be called after the mount point has been successfully
   * started.  (It is the caller's responsibility to perform proper
   * synchronization here with the mount start operation.  This method provides
   * no internal synchronization of its own.)
   */
  fusell::Channel* getFuseChannel() const;

  /**
   * Return the path to the mount point.
   */
  const AbsolutePath& getPath() const;

  /**
   * Get the hash of the currently checked out snapshot.
   */
  Hash getSnapshotID() const {
    return *currentSnapshot_.rlock();
  }

  /*
   * Return bind mounts that are applied for this mount. These are based on the
   * state of the ClientConfig when this EdenMount was created.
   */
  const std::vector<BindMount>& getBindMounts() const;

  /**
   * Return the ObjectStore used by this mount point.
   *
   * The ObjectStore is guaranteed to be valid for the lifetime of the
   * EdenMount.
   */
  ObjectStore* getObjectStore() const {
    return objectStore_.get();
  }

  /**
   * Return the EdenDispatcher used for this mount.
   */
  EdenDispatcher* getDispatcher() const {
    return dispatcher_.get();
  }

  /**
   * Return the InodeMap for this mount.
   */
  InodeMap* getInodeMap() const {
    return inodeMap_.get();
  }

  const std::shared_ptr<Overlay>& getOverlay() const {
    return overlay_;
  }

  Dirstate* getDirstate() {
    return dirstate_.get();
  }

  folly::Synchronized<Journal>& getJournal() {
    return journal_;
  }

  uint64_t getMountGeneration() const {
    return mountGeneration_;
  }

  const ClientConfig* getConfig() const {
    return config_.get();
  }

  /** Get the TreeInode for the root of the mount. */
  TreeInodePtr getRootInode() const;

  /** Convenience method for getting the Tree for the root of the mount. */
  std::unique_ptr<Tree> getRootTree() const;
  folly::Future<std::unique_ptr<Tree>> getRootTreeFuture() const;

  /**
   * Look up the Inode object for the specified path.
   *
   * This may fail with an InodeError containing ENOENT if the path does not
   * exist, or ENOTDIR if one of the intermediate components along the path is
   * not a directory.
   *
   * This may also fail with other exceptions if something else goes wrong
   * besides the path being invalid (for instance, an error loading data from
   * the ObjectStore).
   */
  folly::Future<InodePtr> getInode(RelativePathPiece path) const;

  /**
   * A blocking version of getInode().
   *
   * @return the InodeBase for the specified path or throws a std::system_error
   *     with ENOENT.
   *
   * TODO: We should switch all callers to use the Future-base API, and remove
   * the blocking API.
   */
  InodePtr getInodeBlocking(RelativePathPiece path) const;

  /**
   * Syntactic sugar for getInode().get().asTreePtr()
   *
   * TODO: We should switch all callers to use the Future-base API, and remove
   * the blocking API.
   */
  TreeInodePtr getTreeInodeBlocking(RelativePathPiece path) const;

  /**
   * Syntactic sugar for getInode().get().asFilePtr()
   *
   * TODO: We should switch all callers to use the Future-base API, and remove
   * the blocking API.
   */
  FileInodePtr getFileInodeBlocking(RelativePathPiece path) const;

  /**
   * Check out the specified commit.
   */
  folly::Future<std::vector<CheckoutConflict>> checkout(
      Hash snapshotHash,
      bool force = false);

  /**
   * Compute differences between the current commit and the working directory
   * state.
   *
   * @param callback This callback will be invoked as differences are found.
   *     Note that the callback methods may be invoked simultaneously from
   *     multiple different threads, and the callback is responsible for
   *     performing synchronization (if it is needed).
   * @param listIgnored Whether or not to inform the callback of ignored files.
   *     When listIgnored to false can speed up the diff computation, as the
   *     code does not need to descend into ignord directories at all.
   */
  folly::Future<folly::Unit> diff(
      InodeDiffCallback* callback,
      bool listIgnored = false);

  /**
   * Reset the state to point to the specified commit, without modifying
   * the working directory contents at all.
   */
  void resetCommit(Hash snapshotHash);

  /**
   * Acquire the rename lock in exclusive mode.
   */
  RenameLock acquireRenameLock();

  /**
   * Acquire the rename lock in shared mode.
   */
  SharedRenameLock acquireSharedRenameLock();

  /**
   * shutdownComplete() will be called by InodeMap when all outstanding Inodes
   * for this mount point have been deleted.
   *
   * This method should only be invoked by InodeMap.
   */
  void shutdownComplete();

 private:
  friend class RenameLock;
  friend class SharedRenameLock;

  // Forbidden copy constructor and assignment operator
  EdenMount(EdenMount const&) = delete;
  EdenMount& operator=(EdenMount const&) = delete;

  /**
   * Private destructor.
   *
   * This should not be invoked by callers directly.  Use the destroy() method
   * above (or the EdenMountDeleter if you plan to store the EdenMount in a
   * std::unique_ptr or std::shared_ptr).
   */
  ~EdenMount();

  std::unique_ptr<ClientConfig> config_;
  std::unique_ptr<InodeMap> inodeMap_;
  std::unique_ptr<EdenDispatcher> dispatcher_;
  std::unique_ptr<fusell::MountPoint> mountPoint_;
  std::unique_ptr<ObjectStore> objectStore_;
  std::shared_ptr<Overlay> overlay_;
  std::unique_ptr<Dirstate> dirstate_;

  /**
   * A mutex around all name-changing operations in this mount point.
   *
   * This includes rename() operations as well as unlink() and rmdir().
   * Any operation that modifies an existing InodeBase's location_ data must
   * hold the rename lock.
   */
  folly::SharedMutex renameMutex_;

  /**
   * The hash of the current snapshot (i.e., commit) that is checked out in
   * this mount point.
   */
  folly::Synchronized<Hash> currentSnapshot_;

  /*
   * Note that this config will not be updated if the user modifies the
   * underlying config files after the ClientConfig was created.
   */
  const std::vector<BindMount> bindMounts_;

  folly::Synchronized<Journal> journal_;

  /**
   * A number to uniquely identify this particular incarnation of this mount.
   * We use bits from the process id and the time at which we were mounted.
   */
  const uint64_t mountGeneration_;
};

/**
 * RenameLock is a holder for an EdenMount's rename mutex.
 *
 * This is primarily useful so it can be forward declared easily,
 * but it also provides a helper method to ensure that it is currently holding
 * a lock on the desired mount.
 */
class RenameLock : public std::unique_lock<folly::SharedMutex> {
 public:
  RenameLock() {}
  explicit RenameLock(EdenMount* mount)
      : std::unique_lock<folly::SharedMutex>{mount->renameMutex_} {}

  bool isHeld(EdenMount* mount) const {
    return owns_lock() && (mutex() == &mount->renameMutex_);
  }
};

/**
 * SharedRenameLock is a holder for an EdenMount's rename mutex in shared mode.
 */
class SharedRenameLock : public std::shared_lock<folly::SharedMutex> {
 public:
  explicit SharedRenameLock(EdenMount* mount)
      : std::shared_lock<folly::SharedMutex>{mount->renameMutex_} {}

  bool isHeld(EdenMount* mount) const {
    return owns_lock() && (mutex() == &mount->renameMutex_);
  }
};

/**
 * EdenMountDeleter acts as a deleter argument for std::shared_ptr or
 * std::unique_ptr.
 */
class EdenMountDeleter {
 public:
  void operator()(EdenMount* mount) {
    mount->destroy();
  }
};
}
}
