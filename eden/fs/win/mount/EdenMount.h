/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/SharedMutex.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/logging/Logger.h>
#include <chrono>
#include <memory>
#include <mutex>
#include <shared_mutex>
#include "ProjectedFsLib.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/model/ParentCommits.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/mount/EdenDispatcher.h"
#include "eden/fs/win/mount/FsChannel.h"
#include "eden/fs/win/utils/Stub.h" // @manual

namespace folly {
class EventBase;
class File;

template <typename T>
class Future;
} // namespace folly

namespace facebook {
namespace eden {

class BindMount;
class CheckoutConfig;
class CheckoutConflict;
class Clock;
class CurrentState;
class DiffContext;
class EdenDispatcher;
class FuseChannel;
class DiffCallback;
class InodeMap;
class MountPoint;
struct InodeMetadata;
class ObjectStore;
class Overlay;
class ServerState;
class Tree;
class UnboundedQueueExecutor;

class RenameLock;
class SharedRenameLock;

/*
 * TODO(puneetk): This file needs some cleaning from old unused stuff and fix
 * the comments.
 */

/**
 * Represents types of keys for some fb303 counters.
 */
enum class CounterName {
  /**
   * Represents count of loaded inodes in the current mount.
   */
  LOADED,
  /**
   * Represents count of unloaded inodes in the current mount.
   */
  UNLOADED
};

/**
 * EdenMount contains all of the data about a specific eden mount point.
 *
 * This contains:
 * - The MountPoint object which manages our FUSE interactions with the kernel.
 * - The ObjectStore object used for retreiving/storing object data.
 * - The Overlay object used for storing local changes (that have not been
 *   committed/snapshotted yet).
 */
class EdenMount {
 public:
  /**
   * Create a shared_ptr to an EdenMount.
   *
   * Create an EdenMount instance Using an EdenMountDeleter.
   * The caller must call initialize() after creating the EdenMount
   * instance.  This is not done implicitly because the graceful
   * restart code needs to take the opportunity to update the InodeMap
   * prior to the logic in initialize() running.
   */
  static std::shared_ptr<EdenMount> create(
      std::unique_ptr<CheckoutConfig> config,
      std::shared_ptr<ObjectStore> objectStore,
      std::shared_ptr<ServerState> serverState,
      std::unique_ptr<Journal> journal);

  /**
   * Destroy the EdenMount.
   *
   * This method generally does not need to be invoked directly, and will
   * instead be invoked automatically by the shared_ptr<EdenMount> returned by
   * create(), once it becomes unreferenced.
   *
   * If the EdenMount has not already been explicitly shutdown(), destroy()
   * will trigger the shutdown().  destroy() blocks until the shutdown is
   * complete, so it is advisable for callers to callers to explicitly trigger
   * shutdown() themselves if they want to ensure that the shared_ptr
   * destruction will not block on this operation.
   */
  void destroy();

  folly::Future<std::tuple<SerializedFileHandleMap, SerializedInodeMap>>
  shutdown(bool doTakeover, bool allowFuseNotStarted = false);

  /**
   * Return the path to the mount point.
   */
  const AbsolutePath& getPath() const;

  /**
   * Get the commit IDs of the working directory's parent commit(s).
   */
  ParentCommits getParentCommits() const {
    return parentInfo_.rlock()->parents;
  }

  /**
   * Return the ObjectStore used by this mount point.
   *
   * The ObjectStore is guaranteed to be valid for the lifetime of the
   * EdenMount.
   */
  const ObjectStore* getObjectStore() const {
    return objectStore_.get();
  }

  /**
   * Return the EdenDispatcher used for this mount.
   */
  const EdenDispatcher* getDispatcher() const {
    return &dispatcher_;
  }

  Journal& getJournal() {
    return *journal_;
  }

  uint64_t getMountGeneration() const {
    return mountGeneration_;
  }

  const CheckoutConfig* getConfig() const {
    return config_.get();
  }

  CurrentState* getCurrentState() const {
    return currentState_.get();
  }

  /**
   * Returns the server's thread pool.
   */
  const std::shared_ptr<UnboundedQueueExecutor>& getThreadPool() const;

  /** Convenience method for getting the Tree for the root of the mount. */
  std::shared_ptr<const Tree> getRootTree() const;
  folly::Future<std::shared_ptr<const Tree>> getRootTreeFuture() const;

  /**
   * Check out the specified commit.
   */
  folly::Future<std::vector<CheckoutConflict>> checkout(
      Hash snapshotHash,
      CheckoutMode checkoutMode = CheckoutMode::NORMAL);

  /**
   * This version of diff is primarily intended for testing.
   * Use diff(DiffCallback* callback, bool listIgnored) instead.
   * The caller must ensure that the DiffContext object ctsPtr points to
   * exists at least until the returned Future completes.
   */
  folly::Future<folly::Unit> diff(const DiffContext* ctxPtr, Hash commitHash)
      const;

  /**
   * Compute differences between the current commit and the working directory
   * state.
   *
   * @param callback This callback will be invoked as differences are found.
   *     Note that the callback methods may be invoked simultaneously from
   *     multiple different threads, and the callback is responsible for
   *     performing synchronization (if it is needed).
   * @param listIgnored Whether or not to inform the callback of ignored files.
   *     When listIgnored is set to false can speed up the diff computation, as
   *     the code does not need to descend into ignored directories at all.
   * @param request This ResposeChannelRequest is passed from the ServiceHandler
   *     and is used to check if the request is still active, because if the
   *     request is no longer active we will cancel this diff operation.
   *
   * @return Returns a folly::Future that will be fulfilled when the diff
   *     operation is complete.  This is marked FOLLY_NODISCARD to
   *     make sure callers do not forget to wait for the operation to complete.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> diff(
      DiffCallback* callback,
      Hash commitHash,
      bool listIgnored = false,
      bool enforceCurrentParent = false,
      apache::thrift::ResponseChannelRequest* FOLLY_NULLABLE request =
          nullptr) const;

  /**
   * Executes diff against commitHash and returns the ScmStatus for the diff
   * operation.
   */
  folly::Future<std::unique_ptr<ScmStatus>> diff(
      Hash commitHash,
      bool listIgnored,
      bool enforceCurrentParent,
      apache::thrift::ResponseChannelRequest* FOLLY_NULLABLE request = nullptr);

  /**
   * Reset the state to point to the specified parent commit(s), without
   * modifying the working directory contents at all.
   */
  void resetParents(const ParentCommits& parents);

  /**
   * Reset the state to point to the specified parent commit, without
   * modifying the working directory contents at all.
   *
   * This is a small wrapper around resetParents() for when the code knows at
   * compile time that it will only ever have a single parent commit on this
   * code path.
   */
  void resetParent(const Hash& parent);

  /**
   * Acquire the rename lock in exclusive mode.
   */
  RenameLock acquireRenameLock();

  /**
   * Acquire the rename lock in shared mode.
   */
  SharedRenameLock acquireSharedRenameLock();

  /**
   * Returns a pointer to a stats instance associated with this mountpoint.
   * Today this is the global stats instance, but in the future it will be
   * a mount point specific instance.
   */
  EdenStats* getStats() const;

  folly::Logger& getStraceLogger() {
    return straceLogger_;
  }

  /**
   * Returns the last checkout time in the Eden mount.
   */
  struct timespec getLastCheckoutTime() const;

  /**
   * Set the last checkout time.
   *
   * This is intended primarily for use in test code.
   */
  void setLastCheckoutTime(std::chrono::system_clock::time_point time);

  /**
   * Returns the key value to an fb303 counter.
   */
  std::string getCounterName(CounterName name);

  struct ParentInfo {
    ParentCommits parents;
  };

  void start();

  void stop();

  uid_t getUid() const {
    return uid_;
  }

  gid_t getGid() const {
    return gid_;
  }

  bool isSafeForInodeAccess() const {
    return true;
  }

  /**
   * Return a new stat structure that has been minimally initialized with
   * data for this mount point.
   *
   * The caller must still initialize all file-specific data (inode number,
   * file mode, size, timestamps, link count, etc).
   */
  struct stat initStatData() const;

 private:
  friend class RenameLock;
  friend class SharedRenameLock;
  class JournalDiffCallback;

  /**
   * The current running state of the EdenMount.
   *
   * For now this primarily tracks the status of the shutdown process.
   * In the future we may want to add other states to also track the status of
   * the actual mount point in the kernel.  (e.g., a "STARTING" state before
   * RUNNING for when the kernel mount point has not been fully set up yet, and
   * an "UNMOUNTING" state if we have requested the kernel to unmount the mount
   * point and that has not completed yet.  UNMOUNTING would occur between
   * RUNNING and SHUT_DOWN.)  One possible downside of tracking
   * STARTING/UNMOUNTING is that not every EdenMount object actually has a FUSE
   * mount.  During unit tests we create EdenMount objects without ever
   * actually mounting them in the kernel.
   */
  enum class State : uint32_t {
    /**
     * Freshly created.
     */
    UNINITIALIZED,

    /*
     *Either not started or stopped.
     */
    NOT_RUNNING,

    /**
     * The EdenMount is running normally.
     */
    RUNNING,

    /**
     * EdenMount::shutdown() has been called, but it is not complete yet.
     */
    SHUTTING_DOWN,

    /*
     * Destroy has been called for this mount
     */

    DESTROYING

  };

  /**
   * Recursive method used for resolveSymlink() implementation
   */
  folly::Future<InodePtr>
  resolveSymlinkImpl(InodePtr pInode, RelativePath&& path, size_t depth) const;

  /**
   * Attempt to transition from expected -> newState.
   * If the current state is expected then the state is set to newState
   * and returns boolean.
   * Otherwise the current state is left untouched and returns false.
   */
  bool doStateTransition(State expected, State newState);

  EdenMount(
      std::unique_ptr<CheckoutConfig> config,
      std::shared_ptr<ObjectStore> objectStore,
      std::shared_ptr<ServerState> serverState,
      std::unique_ptr<Journal> journal);

  // Forbidden copy constructor and assignment operator
  EdenMount(EdenMount const&) = delete;
  EdenMount& operator=(EdenMount const&) = delete;

  folly::Future<TreeInodePtr> createRootInode(
      const ParentCommits& parentCommits);
  FOLLY_NODISCARD folly::Future<folly::Unit> setupDotEden(TreeInodePtr root);
  folly::Future<std::tuple<SerializedFileHandleMap, SerializedInodeMap>>
  shutdownImpl(bool doTakeover);

  std::unique_ptr<DiffContext> createDiffContext(
      DiffCallback* callback,
      bool listIgnored,
      apache::thrift::ResponseChannelRequest* request) const;

  /**
   * Private destructor.
   *
   * This should not be invoked by callers directly.  Use the destroy() method
   * above (or the EdenMountDeleter if you plan to store the EdenMount in a
   * std::unique_ptr or std::shared_ptr).
   */
  ~EdenMount();

  static constexpr int kMaxSymlinkChainDepth = 40; // max depth of symlink chain

  const std::unique_ptr<const CheckoutConfig> config_;

  /**
   * Eden server state shared across multiple mount points.
   */
  std::shared_ptr<ServerState> serverState_;
  std::shared_ptr<ObjectStore> objectStore_;

  EdenDispatcher dispatcher_;

  std::unique_ptr<CurrentState> currentState_;

  /**
   * This is the channel between ProjectedFS and rest of Eden.
   */
  FsChannel fsChannel_;

  /**
   * A mutex around all name-changing operations in this mount point.
   *
   * This includes rename() operations as well as unlink() and rmdir().
   * Any operation that modifies an existing InodeBase's location_ data must
   * hold the rename lock.
   */
  folly::SharedMutex renameMutex_;

  /**
   * The IDs of the parent commit(s) of the working directory.
   *
   * In most circumstances there will only be a single parent, but there
   * will be two parents when in the middle of resolving a merge conflict.
   */

  folly::Synchronized<ParentInfo> parentInfo_;

  /*
   * Note that this config will not be updated if the user modifies the
   * underlying config files after the CheckoutConfig was created.
   */
  const std::vector<BindMount> bindMounts_;

  std::unique_ptr<Journal> journal_;

  /**
   * A number to uniquely identify this particular incarnation of this mount.
   * We use bits from the process id and the time at which we were mounted.
   */
  const uint64_t mountGeneration_;

  /**
   * The path to the unix socket that can be used to address us via thrift
   */
  AbsolutePath socketPath_;

  /**
   * A log category for logging strace-events for this mount point.
   *
   * All FUSE operations to this mount point will get logged to this category.
   * The category name is of the following form: "eden.strace.<mount_path>"
   */
  folly::Logger straceLogger_;

  /**
   * The timestamp of the last time that a checkout operation was performed in
   * this mount.  This is used to initialize the timestamps of newly loaded
   * inodes.  (Since the file contents might have logically been update by the
   * checkout operation.)
   *
   * We store this as a struct timespec rather than a std::chrono::time_point
   * since this is primarily used by FUSE APIs which need a timespec.
   *
   * This is managed with its own Synchronized lock separate from other state
   * since it needs to be accessed when constructing inodes.  This is a very
   * low level lock in our lock ordering hierarchy: No other locks should be
   * acquired while holding this lock.
   */
  folly::Synchronized<struct timespec> lastCheckoutTime_;

  /**
   * The current state of the mount point.
   */
  std::atomic<State> state_{State::UNINITIALIZED};

  /**
   * uid and gid that we'll set as the owners in the stat information
   * returned via initStatData().
   */
  uid_t uid_;
  gid_t gid_;
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
} // namespace eden
} // namespace facebook
