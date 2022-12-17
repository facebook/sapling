/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/CancellationToken.h>
#include <folly/Portability.h>
#include <folly/SharedMutex.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/futures/Future.h>
#include <folly/futures/Promise.h>
#include <folly/futures/SharedPromise.h>
#include <folly/logging/Logger.h>
#include <folly/portability/GFlags.h>
#include <chrono>
#include <memory>
#include <mutex>
#include <optional>
#include <shared_mutex>
#include <stdexcept>
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/CacheHint.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/InodePtrFwd.h"
#include "eden/fs/inodes/InodeTimestamps.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/VirtualInode.h"
#include "eden/fs/model/RootId.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BlobAccess.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/telemetry/ActivityBuffer.h"
#include "eden/fs/telemetry/IActivityRecorder.h"
#include "eden/fs/utils/PathFuncs.h"

#ifndef _WIN32
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/OverlayFileAccess.h"
#include "eden/fs/nfs/Nfsd3.h"
#else
#include "eden/fs/prjfs/PrjfsChannel.h"
#endif

DECLARE_string(edenfsctlPath);

namespace folly {
class EventBase;
class File;

template <typename T>
class Future;
} // namespace folly

namespace facebook::eden {

class BindMount;
class BlobCache;
class CheckoutConfig;
class CheckoutConflict;
class Clock;
class DiffContext;
class EdenConfig;
class FuseChannel;
class FuseDeviceUnmountedDuringInitialization;
class DiffCallback;
class InodeMap;
class MountPoint;
struct InodeMetadata;
template <typename T>
class InodeTable;
using InodeMetadataTable = InodeTable<InodeMetadata>;
class Journal;
class ObjectStore;
class Overlay;
class OverlayFileAccess;
class ServerState;
class Tree;
class TreePrefetchLease;
class UnboundedQueueExecutor;
template <typename T>
class ImmediateFuture;
class TreeEntry;

class RenameLock;
class SharedRenameLock;

/**
 * Represents an inode state transition and the duration it took for the event
 * to occur. Currently this tracks inode loads and inode materializations. This
 * type extends the TraceEventBase class so that events can be added to a
 * tracebus for which an ActivityBuffer subscribes to and stores events from.
 *
 * An inode materialization specifically refers to when a new version of
 * an inode's contents are saved in the overlay while before they referred
 * directly to a source control object. The duration we count for an inode
 * materialization consists of any time spent preparing/collecting file data,
 * writing the data to EdenFS's overlay, and materializing any parent inodes.
 *
 * An inode load refers to fetching state for the inode to store into memory.
 * Note: this is not the same as fetching data content for the inode. Fetching
 * data content from an hg BackingStore in particular is an HgImportTraceEvent.
 *
 * Note, path could be the full path (in the case of inode creations), or,
 * more commonly, just base filenames depending on how much is easily
 * available during the inode event.
 */
struct InodeTraceEvent : TraceEventBase {
  template <typename Path>
  InodeTraceEvent(
      std::chrono::system_clock::time_point startTime,
      InodeNumber ino,
      InodeType inodeType,
      InodeEventType eventType,
      InodeEventProgress progress,
      const Path& path)
      : InodeTraceEvent{startTime, ino, inodeType, eventType, progress} {
    setPath(path.view());
  }

  // Simple accessor that hides the internal memory representation of the trace
  // event's path. Note this could be just the base filename or it could be the
  // full path depending on how much was available and if the event has already
  // been added into the ActivityBuffer.
  std::string getPath() const {
    return path.get();
  }

  // Setter that allocates new memory on the heap and memcpy's a StringPiece's
  // data into the InodeTraceEvent's path attribute
  void setPath(std::string_view stringPath);

  InodeNumber ino;
  InodeType inodeType;
  InodeEventType eventType;
  InodeEventProgress progress;
  std::chrono::microseconds duration;
  // Always null-terminated, and saves space in the trace event structure.
  std::shared_ptr<char[]> path;

 private:
  InodeTraceEvent(
      std::chrono::system_clock::time_point startTime,
      InodeNumber ino,
      InodeType inodeType,
      InodeEventType eventType,
      InodeEventProgress progress);
};

/**
 * Represents types of keys for some fb303 counters.
 */
enum class CounterName {
  /**
   * Represents count of loaded inodes in the current mount.
   */
  INODEMAP_LOADED,
  /**
   * Represents count of unloaded inodes in the current mount.
   */
  INODEMAP_UNLOADED,
  /**
   * Represents the amount of memory used by deltas in the change log
   */
  JOURNAL_MEMORY,
  /**
   * Represents the number of entries in the change log
   */
  JOURNAL_ENTRIES,
  /**
   * Represents the duration of the journal in seconds end to end
   */
  JOURNAL_DURATION,
  /**
   * Represents the maximum deltas iterated over in the Journal's forEachDelta
   */
  JOURNAL_MAX_FILES_ACCUMULATED,

  /**
   * Represents the number of inodes unloaded for this mount by periodic
   * linked inode unloading. This is used as an optimization to prevent inode
   * build up.
   */
  PERIODIC_INODE_UNLOAD,

  /**
   * Represents the number of inodes unloaded for this mount by periodic
   * unlinked inode unloading. This is used on NFS mounts to clean up old
   * inodes.
   */
  PERIODIC_UNLINKED_INODE_UNLOAD

};

/**
 * Contains the uid and gid of the owner of the files in the mount
 */
struct Owner {
  uid_t uid;
  gid_t gid;
};

/**
 * Durations of the various stages of checkout.
 */
struct CheckoutTimes {
  using duration = std::chrono::steady_clock::duration;
  duration didLookupTrees{};
  duration didDiff{};
  duration didAcquireRenameLock{};
  duration didCheckout{};
  duration didFinish{};
};

/**
 * Durations of the various stages of setPathObjectId.
 */
struct SetPathObjectIdTimes {
  using duration = std::chrono::steady_clock::duration;
  duration didLookupTreesOrGetInodeByPath{};
  duration didCheckout{};
  duration didFinish{};
};

struct CheckoutResult {
  std::vector<CheckoutConflict> conflicts;
  CheckoutTimes times;
};

struct SetPathObjectIdResultAndTimes {
  SetPathObjectIdResult result;
  SetPathObjectIdTimes times;
};

struct SetPathObjectIdObjectAndPath {
  RelativePath path;
  ObjectId id;
  ObjectType type;

  std::string toString() const {
    return fmt::format(
        "path={}, objectId={}, type={}",
        path.value(),
        id.asString(),
        convertTypeToString(type));
  }

 private:
  std::string_view convertTypeToString(ObjectType type) const {
    switch (type) {
      case ObjectType::TREE:
        return "tree";
      case ObjectType::REGULAR_FILE:
        return "regular_file";
      case ObjectType::EXECUTABLE_FILE:
        return "executable_file";
      case ObjectType::SYMLINK:
        return "symlink";
    }
  }
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
class EdenMount : public std::enable_shared_from_this<EdenMount> {
 public:
  using State = MountState;

  /**
   * Create a shared_ptr to an EdenMount.
   *
   * The caller must call initialize() after creating the EdenMount to load data
   * required to access the mount's inodes.  No inode-related methods may be
   * called on the EdenMount until initialize() has successfully completed.
   */
  static std::shared_ptr<EdenMount> create(
      std::unique_ptr<CheckoutConfig> config,
      std::shared_ptr<ObjectStore> objectStore,
      std::shared_ptr<BlobCache> blobCache,
      std::shared_ptr<ServerState> serverState,
      std::unique_ptr<Journal> journal,
      std::optional<Overlay::InodeCatalogType> inodeCatalogType = std::nullopt);

  /**
   * Asynchronous EdenMount initialization - post instantiation.
   *
   * If takeover data is specified, it is used to initialize the inode map.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> initialize(
      OverlayChecker::ProgressCallback&& progressCallback = [](auto) {},
      const std::optional<SerializedInodeMap>& takeover = std::nullopt);

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

  /**
   * Shutdown the EdenMount.
   *
   * This should be called *after* calling unmount() (i.e. after the FUSE mount
   * point has been unmounted from the kernel).
   *
   * This cleans up the in-memory data associated with the EdenMount, and waits
   * for all outstanding InodeBase objects to become unreferenced and be
   * destroyed.
   *
   * If doTakeover is true, this function will return populated
   * SerializedFileHandleMap and SerializedInodeMap instances generated by
   * calling FileHandleMap::serializeMap() and InodeMap::shutdown.
   *
   * If doTakeover is false, this function will return default-constructed
   * SerializedFileHandleMap and SerializedInodeMap instances.
   */
  folly::SemiFuture<SerializedInodeMap> shutdown(
      bool doTakeover,
      bool allowFuseNotStarted = false);

  /**
   * Call the umount(2) syscall to tell the kernel to remove this filesystem.
   *
   * After umount(2) succeeds, the following operations happen independently and
   * concurrently:
   *
   * * The future returned by unmount() is fulfilled successfully.
   * * The future returned by getChannelCompletionFuture() is fulfilled.
   *
   * If startChannel() is in progress, unmount() can cancel startChannel().
   *
   * If startChannel() is in progress, unmount() might wait for startChannel()
   * to finish before calling umount(2).
   *
   * If neither startChannel() nor takeoverFuse() has been called, unmount()
   * finishes successfully without calling umount(2). Thereafter, startChannel()
   * and takeoverFuse() will both fail with an EdenMountCancelled exception.
   *
   * unmount() is idempotent: If unmount() has already been called, this
   * function immediately returns a Future which will complete at the same time
   * the original call to unmount() completes.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> unmount();

  /**
   * Get the current state of this mount.
   *
   * Note that the state may be changed by another thread immediately after this
   * method is called, so this method should primarily only be used for
   * debugging & diagnostics.
   */
  State getState() const {
    return state_.load(std::memory_order_acquire);
  }

  /**
   * Check if inode operations can be performed on this EdenMount.
   *
   * This returns false for mounts that are still initializing and do not have
   * their root inode loaded yet. This also returns false for mounts that are
   * shutting down.
   */
  bool isSafeForInodeAccess() const {
    auto state = getState();
    return !(
        state == State::UNINITIALIZED || state == State::INITIALIZING ||
        state == State::SHUTTING_DOWN);
  }

  /**
   * Get the FUSE/NFS/Prjfs channel for this mount point.
   *
   * This should only be called after the mount point has been successfully
   * started.  (It is the caller's responsibility to perform proper
   * synchronization here with the mount start operation.  This method provides
   * no internal synchronization of its own.)
   */
#ifdef _WIN32
  PrjfsChannel* FOLLY_NULLABLE getPrjfsChannel() const;

  /**
   * Set a test channel for this mount point.
   *
   * This should only be used in test to set a fake channel.
   */
  void setTestPrjfsChannel(std::unique_ptr<PrjfsChannel> channel);
#else
  FuseChannel* FOLLY_NULLABLE getFuseChannel() const;
  Nfsd3* FOLLY_NULLABLE getNfsdChannel() const;
#endif

  /**
   * Detect the FUSE/NFS/Prjfs channel for this mount point.
   *
   * This should only be called after the mount point has been successfully
   * started.  (It is the caller's responsibility to perform proper
   * synchronization here with the mount start operation.  This method provides
   * no internal synchronization of its own.)
   *
   * Note these reflect the actually in use Mount Protocol, not what is written
   * on disk to the mount config. If this mount failed to initialize, these
   * boolean functions may all return false and getMountProtocol may return a
   * nullopt.
   */
  bool isFuseChannel() const;
  bool isNfsdChannel() const;
  bool isPrjfsChannel() const;
  bool fsChannelIsInitialized() const;
  std::optional<MountProtocol> getMountProtocol() const;

  /**
   * Wait for all inflight notifications to complete.
   *
   * On Windows, inflight notifications are processed asynchronously and thus
   * the on-disk state of the the repository may differ from the inode state.
   * This ensures that all pending notifications have completed.
   *
   * On macOS and Linux, this immediately return.
   *
   * This can be called from any thread/executor.
   */
  ImmediateFuture<folly::Unit> waitForPendingNotifications() const;

  /**
   * Test if the working copy persist on disk after this mount will be
   * destroyed.
   *
   * This is only true on Windows when using ProjectedFS as files are left
   * around in the working copy after the mount is unmounted.
   */
  constexpr bool isWorkingCopyPersistent() const {
    return folly::kIsWindows;
  }

  ProcessAccessLog& getProcessAccessLog() const;

  /**
   * Return the path to the mount point.
   */
  const AbsolutePath& getPath() const;

  /**
   * Get the RootId of the working directory's parent commit.
   *
   * EdenFS will populate the working copy from this RootId. This is set to the
   * last RootId passed to checkout.
   */
  RootId getCheckedOutRootId() const {
    return parentState_.rlock()->checkedOutRootId;
  }

  /**
   * Get the RootId of the working copy parent commit.
   *
   * This RootId is set when calling resetParent, and should be used when
   * performing diff operation. A checkout operation will also set this.
   */
  RootId getWorkingCopyParent() const {
    return parentState_.rlock()->workingCopyParentRootId;
  }

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
   * Return Eden's blob cache.
   *
   * It is guaranteed to be valid for the lifetime of the EdenMount.
   */
  BlobCache* getBlobCache() const {
    return blobCache_.get();
  }

  /**
   * Return the BlobAccess used by this mount point.
   *
   * The BlobAccess is guaranteed to be valid for the lifetime of the EdenMount.
   */
  BlobAccess* getBlobAccess() {
    return &blobAccess_;
  }

  /**
   * Return the InodeMap for this mount.
   */
  InodeMap* getInodeMap() const {
    return inodeMap_.get();
  }

  /**
   * Return the Overlay for this mount.
   */
  Overlay* getOverlay() const {
    return overlay_.get();
  }

#ifndef _WIN32
  OverlayFileAccess* getOverlayFileAccess() {
    return &overlayFileAccess_;
  }

#endif // !_WIN32

  InodeMetadataTable* getInodeMetadataTable() const;

  /**
   * Return the Journal used by this mount point.
   *
   * The Journal is guaranteed to be valid for the lifetime of the EdenMount.
   */
  Journal& getJournal() {
    return *journal_;
  }

  folly::Synchronized<std::unique_ptr<IActivityRecorder>>&
  getActivityRecorder() {
    return activityRecorder_;
  }

  uint64_t getMountGeneration() const {
    return mountGeneration_;
  }

  std::shared_ptr<const EdenConfig> getEdenConfig() const;

  const CheckoutConfig* getCheckoutConfig() const {
    return checkoutConfig_.get();
  }

  /**
   * Returns the server's thread pool.
   */
  const std::shared_ptr<UnboundedQueueExecutor>& getServerThreadPool() const;

#ifdef _WIN32
  /**
   * Returns the thread pool where directory invalidation need to be performed.
   */
  const std::shared_ptr<UnboundedQueueExecutor>& getInvalidationThreadPool()
      const;
#endif

  /**
   * Returns the Clock with which this mount was configured.
   */
  const Clock& getClock() const {
    return *clock_;
  }

  /**
   * Used for getting the repo name for logging purposes. This is the repo name
   * as specified by the checkout config
   */
  folly::StringPiece getRepoSourceName() const {
    return basename(checkoutConfig_->getRepoSource());
  }

  /** Get the TreeInode for the root of the mount. */
  TreeInodePtr getRootInode() const;

#ifndef _WIN32
  /**
   * Get the inode number for the .eden dir.  Returns an empty InodeNumber
   * prior to the .eden directory being set up.
   */
  InodeNumber getDotEdenInodeNumber() const;
#endif // !_WIN32

  /**
   * Loads and returns the Tree corresponding to the root of the mount's working
   * copy parent (commit hash or root ID). Note that the returned Tree may not
   * corresponding to the mount's current inode structure.
   */
  std::shared_ptr<const Tree> getCheckedOutRootTree() const;

  /**
   * Look up the Tree or TreeEntry for the specified path.
   *
   * When the source control object referenced by the path is a file, a
   * TreeEntry will be returned, a Tree otherwise.
   *
   * This may fail with an InodeError containing ENOENT if the path does not
   * exist, or ENOTDIR if one of the intermediate components along the path is
   * not a directory.
   *
   * This may also fail with other exceptions if something else goes wrong
   * besides the path being invalid (for instance, an error loading data from
   * the ObjectStore).
   */
  ImmediateFuture<std::variant<std::shared_ptr<const Tree>, TreeEntry>>
  getTreeOrTreeEntry(
      RelativePathPiece path,
      const ObjectFetchContextPtr& context) const;

  /**
   * Walk the Tree hierarchy and return a path whose case matches the Tree path
   * components.
   *
   * This is only really useful on Windows where the casing of paths from
   * ProjectedFS is arbitrary but EdenFS must return the proper casing for
   * them. On other platform, this will simply returns the passed in path.
   *
   * This may fail with the same errors as the getTreeOrTreeEntry method.
   */
  ImmediateFuture<RelativePath> canonicalizePathFromTree(
      RelativePathPiece path,
      const ObjectFetchContextPtr& context) const;

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
   *
   * This function is marked slow due to forcing to load inodes that may not
   * have been loaded previously. Loading an Inode is unfortunately both
   * expensive to load (due to writing to the overlay), and may slow down
   * future checkout operations. The method getVirtualInode below should
   * instead be preferred as it doesn't suffer from these pathological cases.
   */
  ImmediateFuture<InodePtr> getInodeSlow(
      RelativePathPiece path,
      const ObjectFetchContextPtr& context) const;

  /**
   * Look up the Inode, Tree, or TreeEntry for the specified path.
   *
   * When the source control object referenced by the path is
   * materialized, an Inode will be returned. If the object is a file,
   * a TreeEntry will be returned, a Tree otherwise.
   *
   * This may fail with a PathBaseError containing ENOENT if the path does not
   * exist, or ENOTDIR if one of the intermediate components along the path is
   * not a directory.
   *
   * This may also fail with other exceptions if something else goes wrong
   * besides the path being invalid (for instance, an error loading data from
   * the ObjectStore).
   */
  ImmediateFuture<VirtualInode> getVirtualInode(
      RelativePathPiece path,
      const ObjectFetchContextPtr& context) const;

  /**
   * Check out the specified commit.
   *
   * This updates the checkedOutRootId as well as the workingCopyParentRootId to
   * the passed in snapshotHash.
   */
  folly::Future<CheckoutResult> checkout(
      const RootId& snapshotHash,
      std::optional<pid_t> clientPid,
      folly::StringPiece thriftMethodCaller,
      CheckoutMode checkoutMode = CheckoutMode::NORMAL);

  /**
   * Chown the repository to the given uid and gid
   */
  folly::Future<folly::Unit> chown(uid_t uid, gid_t gid);

  /**
   * Compute differences between the current commit and the working directory
   * state.
   *
   * @param listIgnored Whether or not to inform the callback of ignored files.
   *     When listIgnored is set to false can speed up the diff computation, as
   *     the code does not need to descend into ignored directories at all.
   * @param enforceCurrentParent Whether or not to return an error if the
   *     specified commitHash does not match the actual current working
   *     directory parent.  If this is false the code will still compute a diff
   *     against the specified commitHash even the working directory parent
   *     points elsewhere, or when a checkout is currently in progress.
   * @param request This ResposeChannelRequest is passed from the ServiceHandler
   *     and is used to check if the request is still active, because if the
   *     request is no longer active we will cancel this diff operation.
   *
   * @return Returns a folly::Future that will be fulfilled when the diff
   *     operation is complete.  This is marked FOLLY_NODISCARD to
   *     make sure callers do not forget to wait for the operation to complete.
   */
  FOLLY_NODISCARD ImmediateFuture<std::unique_ptr<ScmStatus>> diff(
      const RootId& commitHash,
      folly::CancellationToken cancellation,
      bool listIgnored = false,
      bool enforceCurrentParent = true);

  /**
   * Compute the difference between the passed in roots.
   *
   * The order of the roots matters: a file added in toRoot will be returned as
   * ScmFileStatus::ADDED, while if the order of arguments were reversed, it
   * would be returned as ScmFileStatus::REMOVED.
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> diffBetweenRoots(
      const RootId& fromRoot,
      const RootId& toRoot,
      folly::CancellationToken cancellation,
      DiffCallback* callback);

  /**
   * This version of diff is primarily intended for testing.
   * Use diff(DiffCallback* callback, bool listIgnored) instead.
   * The caller must ensure that the DiffContext object ctsPtr points to
   * exists at least until the returned Future completes.
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> diff(
      DiffContext* ctxPtr,
      const RootId& commitHash) const;

  /**
   * Reset the state to point to the specified parent commit, without
   * modifying the working directory contents at all.
   *
   * This set the workingCopyParentRootId to the passed in RootId.
   */
  void resetParent(const RootId& parent);

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

  const folly::Logger& getStraceLogger() const {
    return straceLogger_;
  }

  const std::shared_ptr<ServerState>& getServerState() const {
    return serverState_;
  }

  std::optional<ActivityBuffer<InodeTraceEvent>>& getActivityBuffer() {
    return inodeActivityBuffer_;
  }

  TraceBus<InodeTraceEvent>& getInodeTraceBus() const {
    return *inodeTraceBus_;
  }

  /**
   * Returns the last checkout time in the Eden mount.
   */
  EdenTimestamp getLastCheckoutTime() const;

  /**
   * Set the last checkout time.
   */
  void setLastCheckoutTime(EdenTimestamp time);

  /**
   * Returns true if a checkout is in progress, false otherwise.
   */
  bool isCheckoutInProgress();

  /**
   * Returns the key value to an fb303 counter.
   */
  std::string getCounterName(CounterName name);

  /**
   * Mounts the filesystem in the VFS and spawns worker threads to
   * dispatch the fuse session.
   *
   * Returns a Future that will complete as soon as the filesystem has been
   * successfully mounted, or as soon as the mount fails (state transitions
   * to RUNNING or FUSE_ERROR).
   *
   * If unmount() is called before startChannel() is called, then startChannel()
   * does the following:
   *
   * * startChannel() does not attempt to mount the filesystem
   * * The returned Future is fulfilled with an EdenMountCancelled exception
   *
   * If unmount() is called while startChannel() is in progress, then
   * startChannel() does the following:
   *
   * * The filesystem is unmounted (if it was mounted)
   * * The returned Future is fulfilled with an
   *   FuseDeviceUnmountedDuringInitialization exception
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> startChannel(bool readOnly);

  /**
   * Take over a FUSE channel for an existing mount point.
   *
   * This spins up worker threads to service the existing FUSE channel and
   * returns immediately, or throws an exception on error.
   *
   * If unmount() is called before takeoverFuse() is called, then takeoverFuse()
   * throws an EdenMountCancelled exception.
   *
   * throws a runtime_error if fuse is not supported on this platform.
   */
  void takeoverFuse(FuseChannelData takeoverData);

  /**
   * Takeover an NFSd3 channel for an existing mount point.
   *
   * This starts listening on the socket and the future completes
   * when the channel is completely setup.
   *
   * If unmount() is called before takeoverNfs() is called, then takeoverNfs()
   * throws an EdenMountCancelled exception.
   *
   * throws a runtime_error if NFS is not supported on this platform.
   */
  folly::Future<folly::Unit> takeoverNfs(NfsChannelData connectedSocket);

  /**
   * Obtains a future that will complete once the channel has wound down.
   *
   * This method may be called at any time, but the returned future will only be
   * fulfilled if startChannel() completes successfully.  If startChannel()
   * fails or is never called, the future returned by
   * getChannelCompletionFuture() will never complete.
   */
  FOLLY_NODISCARD folly::Future<TakeoverData::MountInfo>
  getChannelCompletionFuture();

  Owner getOwner() const {
    return *owner_.rlock();
  }

  void setOwner(uid_t uid, gid_t gid) {
    auto owner = owner_.wlock();
    owner->uid = uid;
    owner->gid = gid;
  }

  /**
   * Return a new stat structure that has been minimally initialized with
   * data for this mount point.
   *
   * The caller must still initialize all file-specific data (inode number,
   * file mode, size, timestamps, link count, etc).
   */
  struct stat initStatData() const;

  /**
   * Given a mode_t, return an initial InodeMetadata.  All timestamps are set
   * to the last checkout time and uid and gid are set to the creator of the
   * mount.
   */
  struct InodeMetadata getInitialInodeMetadata(mode_t mode) const;

  /**
   * Return a newly initialized ActivityBuffer<InodeTraceEvent> for the mount if
   * using ActivityBuffers is enabled and return std::nullopt otherwise.
   */
  std::optional<ActivityBuffer<InodeTraceEvent>> initInodeActivityBuffer();

  /**
   * Subscribes inodeActivityBuffer_ to the inodeTraceBus_ in order to read and
   * store InodeTraceEvents into the ActivityBuffer as they occur. In addition,
   * path names for the inodes are calculated here outside of the critical path
   * of the inode event in order to be displayed in the eden inode tracing CLI.
   *
   * Note: subscribers will acquire the InodeMap's data_ and an InodeBase's
   * location_ lock to calculate paths for inodes. However, we must ensure
   * subscribers NEVER aquire EdenMount's Rename or a TreeInode's contents_
   * lock since inode events can still be published to the inode tracebus
   * holding those locks.
   */
  void subscribeInodeActivityBuffer();

  /**
   * Helper function to publish a new InodeTraceEvent to the mount's
   * inodeTraceBus for telemetry. Used in FileInode, TreeInode, and InodeMap.
   * This function is marked noexcept and is guaranteed to never throw an
   * exception. If tracebus fails (i.e. due to being out of memory), then this
   * exception is caught and telemetry is lost.
   *
   * Note: we must make sure to NEVER call this while holding the InodeMap's
   * data_ lock or an InodeBase's location_ lock since subscribers will also
   * attempt to acquire those locks, causing a deadlock if capacity is reached
   * and tracebus starts to block.
   */
  void publishInodeTraceEvent(InodeTraceEvent&& event) noexcept;

  /**
   * mount any configured bind mounts.
   * This requires that the filesystem already be mounted, and must not
   * be called in the context of a fuseWorkerThread().
   */
  FOLLY_NODISCARD folly::SemiFuture<folly::Unit> performBindMounts();

  FOLLY_NODISCARD folly::Future<folly::Unit> addBindMount(
      RelativePathPiece repoPath,
      AbsolutePathPiece targetPath,
      const ObjectFetchContextPtr& context);
  FOLLY_NODISCARD folly::Future<folly::Unit> removeBindMount(
      RelativePathPiece repoPath);

  /**
   * Ensures the path `fromRoot` is a directory. If it is not, then it creates
   * subdirectories until it is. If creating a subdirectory fails, it throws an
   * exception. Returns the TreeInodePtr to the directory.
   */
  FOLLY_NODISCARD ImmediateFuture<TreeInodePtr> ensureDirectoryExists(
      RelativePathPiece fromRoot,
      const ObjectFetchContextPtr& context);

  /**
   * Request to start a new tree prefetch.
   *
   * Returns a new TreePrefetchLease if you can start a new prefetch, or
   * std::nullopt if there are too many prefetches already in progress and a new
   * one should not be started.  If a TreePrefetchLease object is returned the
   * caller should hold onto it until the prefetch is complete.  When the
   * TreePrefetchLease is destroyed this will inform the EdenMount that the
   * prefetch has finished.
   */
  FOLLY_NODISCARD std::optional<TreePrefetchLease> tryStartTreePrefetch(
      TreeInodePtr treeInode,
      const ObjectFetchContext& context);

  /**
   * Lease to be held for the duration of a background GC.
   *
   * Only a single background GC can be running at a given time.
   */
  class WorkingCopyGCLease {
   public:
    explicit WorkingCopyGCLease(
        std::atomic<bool>* gcRunning,
        TreeInodePtr inode)
        : gcRunning_{gcRunning}, inode_{std::move(inode)} {}

    ~WorkingCopyGCLease() {
      if (inode_) {
        gcRunning_->store(false, std::memory_order_release);
      }
    }

    WorkingCopyGCLease(const WorkingCopyGCLease&) = delete;
    WorkingCopyGCLease& operator=(const WorkingCopyGCLease&) = delete;
    WorkingCopyGCLease(WorkingCopyGCLease&&) = default;
    WorkingCopyGCLease& operator=(WorkingCopyGCLease&&) = default;

   private:
    std::atomic<bool>* gcRunning_;
    // Store the inode for the duration of the GC, this ensures that the mount
    // cannot be unmounted and thus that gcRunning_ will live for at least as
    // long as the lease.
    TreeInodePtr inode_;
  };

  /**
   * Attempt to start a background working copy GC.
   *
   * The returned lease must be held for the duration of the GC to ensure that
   * no other concurrent background GC can be started.
   *
   * This returns a std::nullopt if a background GC is already in progress.
   */
  std::optional<WorkingCopyGCLease> tryStartWorkingCopyGC(TreeInodePtr inode);

  /**
   * Get a weak_ptr to this EdenMount object. EdenMounts are stored as shared
   * pointers inside of EdenServer's MountList.
   */
  std::weak_ptr<EdenMount> getWeakMount() {
    return weak_from_this();
  }

  /**
   * Graft a tree or blob to a path. Returns a folly::Future that will be
   * fulfilled when the setPathObjectId operation is complete. The return result
   * include conflicts if any.
   *
   * CheckoutMode is similar to checkout:
   * 1. In NORMAL mode, new tree or blob will be increamentally added to an Eden
   * mount. The operation will not continue if any conflicts were found.
   * 2. In FORCE mode, only new tree will exist after the operation and any
   * other contents will disappear
   * 3. In DRYRUN mode, no action action will be executed.
   */
  FOLLY_NODISCARD ImmediateFuture<SetPathObjectIdResultAndTimes>
  setPathsToObjectIds(
      std::vector<SetPathObjectIdObjectAndPath> objects,
      CheckoutMode checkoutMode,
      const ObjectFetchContextPtr& context);

  /**
   * Should only be called by the mount contructor. We decide wether this
   * mount should use nfs at construction time and do not change the decision.
   * This is so that we can consitently determine if we are determining if we
   * are using an nfs mount without checking if the channel is an NFS mount.
   * Needed because the InodeMap which is a dependency of ourselves needs to be
   * NFS aware. We don't want a dependency inversion where the inode map relies
   * on the mount to determine if its an NFS inode map.
   */
  bool shouldUseNFSMount() {
#ifndef _WIN32
    return getEdenConfig()->enableNfsServer.getValue() &&
        getCheckoutConfig()->getMountProtocol() == MountProtocol::NFS;
#endif
    return false;
  }
  /**
   * Clear the fs reference count for all stale inodes. Stale inodes are those
   * that have been unlinked and not recently referenced.
   *
   * "referenced" means atime changed on the inodes. This is not a perfect
   * measure as GETATTR calls do not update atime. We might want to use a
   * "referenced" time instead that we update on every inode access.
   *
   * "recently" means 10s by default and is controled by
   * postCheckoutDelayToUnloadInodes.
   */
  void forgetStaleInodes();

  /**
   * If we have a FUSE or NFS channel, flush all invalidations we sent to the
   * kernel This will ensure that other processes will see up-to-date data once
   * we return.
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> flushInvalidations();

 private:
  friend class RenameLock;
  friend class SharedRenameLock;
  class JournalDiffCallback;

  /**
   * Attempt to transition from expected -> newState.
   * If the current state is expected then the state is set to newState
   * and returns boolean.
   * Otherwise the current state is left untouched and returns false.
   */
  FOLLY_NODISCARD bool tryToTransitionState(State expected, State newState);

  /**
   * Transition from expected -> newState.
   *
   * Throws an error if the current state does not match the expected state.
   */
  void transitionState(State expected, State newState);

  /**
   * Transition from the STARTING state to the FUSE_ERROR state.
   *
   * Preconditions:
   * - `getState()` is STARTING or DESTROYING or SHUTTING_DOWN or SHUT_DOWN.
   *
   * Postconditions:
   * - If `getState()` was STARTING, `getState()` is now FUSE_ERROR.
   * - If `getState()` was not STARTING, `getState()` is unchanged.
   *
   * TODO: we should make this a bit more channel type agnostic.
   */
  void transitionToFuseInitializationErrorState();

  /**
   * Returns overlay type based on settings.
   */
  Overlay::InodeCatalogType getInodeCatalogType(
      std::optional<Overlay::InodeCatalogType> inodeCatalogType);

  EdenMount(
      std::unique_ptr<CheckoutConfig> checkoutConfig,
      std::shared_ptr<ObjectStore> objectStore,
      std::shared_ptr<BlobCache> blobCache,
      std::shared_ptr<ServerState> serverState,
      std::unique_ptr<Journal> journal,
      std::optional<Overlay::InodeCatalogType> inodeCatalogType = std::nullopt);

  // Forbidden copy constructor and assignment operator
  EdenMount(EdenMount const&) = delete;
  EdenMount& operator=(EdenMount const&) = delete;

  TreeInodePtr createRootInode(std::shared_ptr<const Tree> tree);

  FOLLY_NODISCARD ImmediateFuture<folly::Unit> setupDotEden(TreeInodePtr root);

  folly::SemiFuture<SerializedInodeMap> shutdownImpl(bool doTakeover);

  /**
   * Create a DiffContext to be passed through the TreeInode diff codepath. This
   * will be used to record differences through the callback (in which
   * listIgnored determines if ignored files will be reported in the callback)
   * and houses the thrift request in order to check to see if the diff() should
   * be short circuited
   */
  std::unique_ptr<DiffContext> createDiffContext(
      DiffCallback* callback,
      folly::CancellationToken cancellation,
      bool listIgnored = false) const;

  /**
   * This accepts a callback which will be invoked as differences are found.
   * Note that the callback methods may be invoked simultaneously from multiple
   * different threads, and the callback is responsible for performing
   * synchronization (if it is needed). It will be packaged into a DiffContext
   * and passed through the TreeInode diff() codepath
   */
  FOLLY_NODISCARD ImmediateFuture<folly::Unit> diff(
      DiffCallback* callback,
      const RootId& commitHash,
      bool listIgnored,
      bool enforceCurrentParent,
      folly::CancellationToken cancellation) const;

  /**
   * Signal to unmount() that fuseMount() or takeoverFuse() has started.
   *
   * beginMount() returns a reference to
   * *mountingUnmountingState_->channelMountPromise. To signal that the
   * fuseMount() has completed, set the promise's value (or exception) without
   * mountingUnmountingState_'s lock held.
   *
   * If unmount() was called in the past, beginMount() throws
   * EdenMountCancelled.
   *
   * Preconditions:
   * - `beginMount()` has not been called before.
   */
  FOLLY_NODISCARD folly::Promise<folly::Unit>& beginMount();

#ifdef _WIN32
  using ChannelStopData = PrjfsChannel::StopData;
#else
  using FuseStopData = FuseChannel::StopData;
  using NfsdStopData = Nfsd3::StopData;
  using ChannelStopData = std::variant<FuseStopData, NfsdStopData>;
#endif

  using StopFuture = folly::SemiFuture<ChannelStopData>;

  /**
   * Open the platform specific device and mount it.
   */
  folly::Future<folly::Unit> channelMount(bool readOnly);

  /**
   * Once the channel has been initialized, set up callbacks to clean up
   * correctly when it shuts down.
   */
  void channelInitSuccessful(EdenMount::StopFuture&& channelCompleteFuture);

  void preparePostChannelCompletion(
      EdenMount::StopFuture&& channelCompleteFuture);

  /**
   * Private destructor.
   *
   * This should not be invoked by callers directly.  Use the destroy() method
   * above (or the EdenMountDeleter if you plan to store the EdenMount in a
   * std::unique_ptr or std::shared_ptr).
   */
  ~EdenMount();

  friend class TreePrefetchLease;
  void treePrefetchFinished() noexcept;

  static constexpr int kMaxSymlinkChainDepth = 40; // max depth of symlink chain

  const std::unique_ptr<const CheckoutConfig> checkoutConfig_;

  /**
   * A promise associated with the future returned from
   * EdenMount::getChannelCompletionFuture() that completes when the
   * fuseChannel has no work remaining and can be torn down.
   * The future yields the underlying fuseDevice descriptor; it can
   * be passed on during graceful restart or simply closed if we're
   * unmounting and shutting down completely.  In the unmount scenario
   * the device should be closed prior to calling EdenMount::shutdown()
   * so that the subsequent privilegedFuseUnmount() call won't block
   * waiting on us for a response.
   */
  folly::Promise<TakeoverData::MountInfo> channelCompletionPromise_;

  /**
   * Eden server state shared across multiple mount points.
   */
  std::shared_ptr<ServerState> serverState_;

#ifdef _WIN32
  /**
   * On Windows, directory invalidation will run on this executor.
   */
  std::shared_ptr<UnboundedQueueExecutor> invalidationExecutor_;
#endif

  /**
   * Should the created mount use NFS (only currently supported on Linux and
   * Windows). We calculate this when the mount is created based on the
   * underlying dynamic configuration.
   */
  bool shouldUseNFSMount_;

  std::unique_ptr<InodeMap> inodeMap_;

  std::shared_ptr<ObjectStore> objectStore_;
  std::shared_ptr<BlobCache> blobCache_;
  BlobAccess blobAccess_;
  std::shared_ptr<Overlay> overlay_;

#ifndef _WIN32
  OverlayFileAccess overlayFileAccess_;
#endif // !_WIN32
  InodeNumber dotEdenInodeNumber_{};

  /**
   * A mutex around all name-changing operations in this mount point.
   *
   * This includes rename() operations as well as unlink() and rmdir().
   * Any operation that modifies an existing InodeBase's location_ data must
   * hold the rename lock.
   */
  folly::SharedMutex renameMutex_;

  struct ParentCommitState {
    // RootId that the working copy is checked out to
    RootId checkedOutRootId;
    std::shared_ptr<const Tree> checkedOutRootTree;
    // RootId that the working copy is reset to. This is usually the same as
    // checkedOutRootId, except when a reset is done, in which case it will
    // differ.
    RootId workingCopyParentRootId;
    bool checkoutInProgress = false;
    std::optional<std::tuple<RootId, RootId>> checkoutOriginalTrees;
    std::optional<pid_t> checkoutPid;
  };

  /**
   * The IDs of the parent commit of the working directory.
   */
 public:
  using ParentLock = folly::Synchronized<ParentCommitState>;

 private:
  ParentLock parentState_;

  std::unique_ptr<Journal> journal_;
  folly::Synchronized<std::unique_ptr<IActivityRecorder>> activityRecorder_;

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
   */
  std::atomic<EdenTimestamp> lastCheckoutTime_;

  struct MountingUnmountingState {
    bool channelMountStarted() const noexcept;
    bool channelUnmountStarted() const noexcept;

    /**
     * Whether or not the mount(2) syscall has been called (via fuseMount).
     *
     * Use this promise to wait for fuseMount to finish.
     *
     * * Empty optional: fuseMount/mount(2) has not been called yet.
     *   (startChannel/fuseMount can be called.)
     * * Unfulfilled: fuseMount is in progress.
     * * Fulfilled with Unit: fuseMount completed successfully (via
     *   startChannel), or we took over the FUSE device from another process
     *   (via takeoverFuse). (startChannel or takeoverFuse can still be in
     *   progress.)
     * * Fulfilled with error: fuseMount failed, or fuseMount was cancelled.
     *
     * The state of this variable might not reflect whether the file system is
     * mounted. For example, if this promise is fulfilled with Unit, then
     * umount(8) is called by another process, the file system will not be
     * mounted.
     */
    std::optional<folly::Promise<folly::Unit>> channelMountPromise;

    /**
     * Whether or not unmount has been called.
     *
     * * Empty optional: unmount has not been called yet. (unmount can be
     *   called.)
     * * Unfulfilled: unmount is in progress, either waiting for a concurrent
     *   fuseMount to complete or waiting for fuseUnmount to complete.
     * * Fulfilled with Unit: unmount was called. fuseUnmount completed
     *   successfully, or fuseMount was never called for this EdenMount.
     * * Fulfilled with error: unmount was called, but fuseUnmount failed.
     *
     * The state of this variable might not reflect whether the file system is
     * unmounted.
     */
    std::optional<folly::SharedPromise<folly::Unit>> channelUnmountPromise;
  };

  folly::Synchronized<MountingUnmountingState> mountingUnmountingState_;

  /**
   * The current state of the mount point.
   */
  std::atomic<State> state_{State::UNINITIALIZED};

  /**
   * uid and gid that we'll set as the owners in the stat information
   * returned via initStatData().
   */
  folly::Synchronized<Owner> owner_;

  /**
   * The number of tree prefetches in progress for this mount point.
   */
  std::atomic<uint64_t> numPrefetchesInProgress_{0};

  /**
   * Whether a periodic working copy GC is ongoing for this mount.
   */
  std::atomic<bool> workingCopyGCInProgress_{false};

  /**
   * Fixed sized buffer containing recent inode events that have occured within
   * EdenFS. Used in the retroactive version of the eden inode trace command.
   *
   * The initialization of this buffer depends on serverState_ being
   * intitialized to get eden config information, so inodeActivityBuffer_ is
   * ordered after serverState_ in this header file. Also, inodeTraceBus_
   * subscriptions publish to inodeActivityBuffer_ so inodeActivityBuffer_ is
   * ordered before inodeTraceBus_.
   */
  std::optional<ActivityBuffer<InodeTraceEvent>> inodeActivityBuffer_;

  std::shared_ptr<TraceBus<InodeTraceEvent>> inodeTraceBus_;

  // Handle for inodeTraceBus subscription
  struct InodeTraceHandle {
    TraceSubscriptionHandle<InodeTraceEvent> subHandle;
  };

  std::shared_ptr<InodeTraceHandle> inodeTraceHandle_;

#ifdef _WIN32
  /**
   * This is the channel between ProjectedFS and rest of Eden.
   */
  std::unique_ptr<PrjfsChannel> channel_;

#else
  using FuseChannelVariant = std::unique_ptr<FuseChannel, FuseChannelDeleter>;
  using NfsdChannelVariant = std::unique_ptr<Nfsd3>;

  /**
   * The associated fuse channel to the kernel.
   */
  std::variant<std::monostate, FuseChannelVariant, NfsdChannelVariant> channel_;
#endif // !_WIN32

  /**
   * The clock.  This is also available as serverState_->getClock().
   * We still keep it as a separate member variable for now so that getClock()
   * can be inline without having to include ServerState.h in this file.
   */
  std::shared_ptr<Clock> clock_;
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

class EdenMountCancelled : public std::runtime_error {
 public:
  explicit EdenMountCancelled();
};

} // namespace facebook::eden

template <>
struct fmt::formatter<facebook::eden::InodeTraceEvent> {
  constexpr auto parse(format_parse_context& ctx) {
    return ctx.begin();
  }

  template <typename Context>
  auto format(const facebook::eden::InodeTraceEvent& event, Context& ctx)
      const {
    return fmt::format_to(
        ctx.out(),
        "Inode Trace Event: {} {} {} {} {} {} {}",
        event.systemTime.time_since_epoch().count(),
        event.eventType == facebook::eden::InodeEventType::MATERIALIZE ? "M"
                                                                       : "L",
        event.progress == facebook::eden::InodeEventProgress::START
            ? "Start"
            : (event.progress == facebook::eden::InodeEventProgress::END
                   ? "End"
                   : "Fail"),
        event.duration.count(),
        event.ino.getRawValue(),
        event.inodeType == facebook::eden::InodeType::TREE ? "Tree" : "File",
        event.getPath());
  }
};
