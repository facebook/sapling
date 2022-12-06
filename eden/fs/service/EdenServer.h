/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

#include <folly/Executor.h>
#include <folly/File.h>
#include <folly/Portability.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/futures/SharedPromise.h>
#include <condition_variable>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/service/PeriodicTask.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IActivityRecorder.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/PathMap.h"

DECLARE_bool(takeover);

namespace cpptoml {
class table;
} // namespace cpptoml

namespace apache {
namespace thrift {
class ThriftServer;
}
} // namespace apache

namespace folly {
class EventBase;
}

namespace facebook::eden {

class BackingStore;
class HgQueuedBackingStore;
class IHiveLogger;
class BlobCache;
class TreeCache;
class Dirstate;
class EdenServiceHandler;
class LocalStore;
class MountInfo;
struct SessionInfo;
class StartupLogger;
class UserInfo;
struct INodePopulationReport;

#ifndef _WIN32
class TakeoverServer;
#endif

/**
 * To avoid EdenServer having a build dependency on every type of BackingStore
 * supported by EdenFS, this layer of indirection allows unit tests to
 * explicitly register a restricted subset.
 */
class BackingStoreFactory {
 public:
  /**
   * The set of parameters a BackingStore's constructor might want. This struct
   * will only be constructed on the stack and will only live for the duration
   * of the createBackingStore call.
   */
  struct CreateParams {
    folly::StringPiece name;
    ServerState* serverState;
    const std::shared_ptr<LocalStore>& localStore;
    const std::shared_ptr<EdenStats>& sharedStats;
    const CheckoutConfig& config;
  };

  virtual ~BackingStoreFactory() = default;

  virtual std::shared_ptr<BackingStore> createBackingStore(
      BackingStoreType type,
      const CreateParams& params) = 0;
};

/*
 * EdenServer contains logic for running the Eden main loop.
 *
 * It performs locking to ensure only a single EdenServer instance is running
 * for a particular location, then starts the thrift management server
 * and the fuse session.
 *
 * The general usage model to run an EdenServer is:
 * - Call prepare().
 * - Run the thrift server.  The server object can be obtained by calling
 *   getServer().  When the thrift server stops this indicates the EdenServer
 *   is done and should be shut down.
 * - Call performCleanup() to let the EdenServer shut down.  This includes
 *   unmounting FUSE mounts or, if a graceful restart was requested,
 *   transferring state to the new process.
 *
 * These are 3 separate steps to provide the caller with flexibility around
 * exactly how they drive the thrift server object.
 */
class EdenServer : private TakeoverHandler {
 public:
  enum class RunState {
    STARTING,
    RUNNING,
    SHUTTING_DOWN,
  };

  using MountList = std::vector<std::shared_ptr<EdenMount>>;

  EdenServer(
      std::vector<std::string> originalCommandLine,
      UserInfo userInfo,
      SessionInfo sessionInfo,
      std::unique_ptr<PrivHelper> privHelper,
      std::shared_ptr<const EdenConfig> edenConfig,
      ActivityRecorderFactory activityRecorderFactory,
      BackingStoreFactory* backingStoreFactory,
      std::shared_ptr<IHiveLogger> hiveLogger,
      std::string version = std::string{});

  virtual ~EdenServer();

  /**
   * Get the server's current status.
   *
   * This is primarily used for diagnostic purposes.
   * Note that the status may change immediately after this method returns, so
   * the value may be out of date by the time the caller can use it.
   */
  RunState getStatus() const {
    return runningState_.rlock()->state;
  }

  /**
   * Prepare to run the EdenServer.
   *
   * This acquires the lock on the eden directory, prepares the thrift server to
   * run, and begins remounting configured mount points.
   *
   * Most of the preparation occurs synchronously before prepare() returns,
   * however a few steps complete asynchronously.  The status of the
   * asynchronous preparation steps is tracked in the returned Future object.
   *
   * The returned future will complete until the EdenServer is running
   * successfully and accepting thrift connections and when all mount points
   * have been remountd.
   *
   * If an error occurs remounting some mount points the Future will complete
   * with an exception, but the server will still continue to run.  Everything
   * will be running normally except for the mount points that failed to be
   * remounted.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepare(
      std::shared_ptr<StartupLogger> logger);

  /**
   * Run the EdenServer.
   */
  void serve() const;

#ifndef _WIN32
  /**
   * Recover the EdenServer after a failed takeover request.
   *
   * This function is very similar to prepare() implementation-wise,
   * but uses a TakeoverData object from a failed takeover request
   * to recover itself.
   *
   * This function resets the TakeoverServer, resets the shutdownFuture, and
   * sets the state to RUNNING
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> recover(TakeoverData&& data);
#endif // _WIN32

  /**
   * Shut down the EdenServer after it has stopped running.
   *
   * This should be called after the EdenServer's thrift server has returned
   * from its serve loop.
   *
   * If a graceful restart has been triggered performCleanup() will stop
   * processing new FUSE requests and transfer state to the new process.
   * Otherwise performCleanup() will unmount and shutdown all currently running
   * mounts.
   */
  bool performCleanup();

  /**
   * Close the backingStore and the localStore.
   */
  void closeStorage() override;

  /**
   * Stops this server, which includes the underlying Thrift server.
   *
   * This may be called from any thread while a call to run() is outstanding,
   * and will cause run() to return.
   */
  void stop();

  /**
   * Request to shutdown the server for a graceful restart operation,
   * allowing a remote process to take over the existing mount points.
   *
   * This pauses FUSE I/O processing, writes filesystem state to disk,
   * and returns the FUSE file descriptors for each mount.  This allows the
   * FUSE FDs to be handed off to a new eden instance so it can take over
   * existing mount points with minimal disruption to other processes using the
   * mounts.
   *
   * Returns a Future that will return a map of (mount path -> FUSE fd)
   */
  folly::Future<TakeoverData> startTakeoverShutdown() override;

  /**
   * Mount and return an EdenMount.
   */
  FOLLY_NODISCARD folly::Future<std::shared_ptr<EdenMount>> mount(
      std::unique_ptr<CheckoutConfig> initialConfig,
      bool readOnly,
      OverlayChecker::ProgressCallback&& progressCallback = [](auto) {},
      std::optional<TakeoverData::MountInfo>&& optionalTakeover = std::nullopt);

  /**
   * Takeover a mount from another eden instance
   */
  FOLLY_NODISCARD folly::Future<std::shared_ptr<EdenMount>> takeoverMount(
      TakeoverData::MountInfo&& takeover);

  /**
   * Unmount an EdenMount.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> unmount(
      AbsolutePathPiece mountPath);

  /**
   * Unmount all mount points maintained by this server, and wait for them to
   * be completely unmounted.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> unmountAll();

  /**
   * Stop all mount points maintained by this server so that they can then be
   * transferred to a new edenfs process to perform a graceful restart.
   */
  FOLLY_NODISCARD folly::Future<TakeoverData> stopMountsForTakeover(
      folly::Promise<std::optional<TakeoverData>>&& takeoverPromise);

  const std::shared_ptr<EdenServiceHandler>& getHandler() const {
    return handler_;
  }
  const std::shared_ptr<apache::thrift::ThriftServer>& getServer() const {
    return server_;
  }

  /**
   * Get the list of mount points.
   *
   * The returned list excludes mount points that are still in the process of
   * initializing.  This is the behavior desired by most callers, as no access
   * to inode information is allowed yet on initializing mount points.
   *
   * Mount points in the returned list may be in the process of shutting down.
   * (Even if we attempted to return only running mount points, they may
   * transition to shutting down before the caller can access them.)
   */
  MountList getMountPoints() const;

  /**
   * Get all mount points, including mounts that are currently initializing.
   */
  MountList getAllMountPoints() const;

  /**
   * Look up an EdenMount by the path where it is mounted.
   *
   * Throws an EdenError if no mount exists with the specified path, or if the
   * mount is still initializing and is not ready for inode operations yet.
   */
  std::shared_ptr<EdenMount> getMount(AbsolutePathPiece mountPath) const;

  folly::Future<CheckoutResult> checkOutRevision(
      AbsolutePathPiece mountPath,
      std::string& rootHash,
      std::optional<folly::StringPiece> rootHgManifest,
      std::optional<pid_t> clientPid,
      folly::StringPiece callerName,
      CheckoutMode checkoutMode);

  std::shared_ptr<LocalStore> getLocalStore() const {
    return localStore_;
  }

  const std::shared_ptr<BlobCache>& getBlobCache() const {
    return blobCache_;
  }

  const std::shared_ptr<TreeCache>& getTreeCache() const {
    return treeCache_;
  }

  /**
   * Look up the BackingStore object for the specified repository type+name.
   *
   * EdenServer maintains an internal cache of all known BackingStores,
   * so that multiple mount points that use the same repository can
   * share the same BackingStore object.
   *
   * If this is the first time this given (type, name) has been used, a new
   * BackingStore object will be created and returned.  Otherwise this will
   * return the existing BackingStore that was previously created.
   */
  std::shared_ptr<BackingStore> getBackingStore(
      BackingStoreType type,
      folly::StringPiece name,
      const CheckoutConfig& config);

  AbsolutePathPiece getEdenDir() const {
    return edenDir_.getPath();
  }

  const std::shared_ptr<ServerState>& getServerState() const {
    return serverState_;
  }

  const std::chrono::time_point<std::chrono::steady_clock> getStartTime()
      const {
    return startTime_;
  }

  const std::string& getVersion() const {
    return version_;
  }

  std::shared_ptr<EdenStats> getSharedStats() {
    return std::shared_ptr<EdenStats>(serverState_, getStats());
  }

  EdenStats* getStats() {
    return &serverState_->getStats();
  }

  /**
   * Returns a ActivityRecorder appropriate for the Eden build.
   */
  std::unique_ptr<IActivityRecorder> makeActivityRecorder(
      std::shared_ptr<EdenMount> edenMount) {
    return activityRecorderFactory_(std::move(edenMount));
  }

  /**
   * Flush all thread-local stats to the main ServiceData object.
   *
   * Thread-local counters are normally flushed to the main ServiceData once
   * a second.  flushStatsNow() can be used to flush thread-local counters on
   * demand, in addition to the normal once-a-second flush.
   *
   * This is mainly useful for unit and integration tests that want to ensure
   * they see up-to-date counter information without waiting for the normal
   * flush interval.
   */
  void flushStatsNow();

  /**
   * Reload the configuration files from disk.
   *
   * The configuration files are automatically reloaded from disk periodically
   * (controlled by the "config:reload-interval" setting in the config file).
   *
   * This method can be invoked to immediately force a config reload,
   * independently of the configured interval.
   */
  void reloadConfig();

  /**
   * Check to make sure that our lock file is still valid.
   */
  void checkLockValidity();

  /**
   * Get the main thread's EventBase.
   *
   * Callers can use this for scheduling work to be run in the main thread.
   */
  folly::EventBase* getMainEventBase() const {
    return mainEventBase_;
  }

  /**
   * Look up all BackingStores
   *
   * EdenServer maintains an internal cache of all known BackingStores,
   * so that multiple mount points that use the same repository can
   * share the same BackingStore object.
   *
   */
  std::unordered_set<std::shared_ptr<BackingStore>> getBackingStores();

  /**
   * Look up all BackingStores which are HgQueuedBackingStore
   *
   * EdenServer maintains an internal cache of all known BackingStores,
   * so that multiple mount points that use the same repository can
   * share the same BackingStore object.
   *
   */
  std::unordered_set<std::shared_ptr<HgQueuedBackingStore>>
  getHgQueuedBackingStores();

  /**
   * Schedule `fn` to run on the main server event base when the `timeout`
   * expires. This does not block until `fn` is scheduled.
   *
   * `fn` will either run before the event base is completely destroyed or
   * not at all.
   *
   * Must be called only from the mainEventBase_ thread.
   */
  void scheduleCallbackOnMainEventBase(
      std::chrono::milliseconds timeout,
      std::function<void()> fn);

  /**
   * Returns the number of in progress checkouts that EdenFS is aware of
   */
  size_t enumerateInProgressCheckouts() {
    size_t numActive = 0;
    auto mountPoints = mountPoints_->rlock();
    for (auto& entry : *mountPoints) {
      auto& info = entry.second;
      numActive += info.edenMount->isCheckoutInProgress() ? 1 : 0;
    }
    return numActive;
  }

 private:
  // Struct to store EdenMount along with SharedPromise that is set
  // during unmount to allow synchronization between unmountFinished
  // and unmount functions.
  struct EdenMountInfo {
    std::shared_ptr<EdenMount> edenMount;
    folly::SharedPromise<folly::Unit> unmountPromise;
    std::optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;

    explicit EdenMountInfo(const std::shared_ptr<EdenMount>& mount)
        : edenMount(mount),
          unmountPromise(folly::SharedPromise<folly::Unit>()) {}
  };

  template <void (EdenServer::*MemberFn)()>
  class PeriodicFnTask : public PeriodicTask {
   public:
    using PeriodicTask::PeriodicTask;
    void runTask() override {
      (getServer()->*MemberFn)();
    }
  };

  using BackingStoreKey = std::pair<BackingStoreType, std::string>;
  using BackingStoreMap =
      std::unordered_map<BackingStoreKey, std::shared_ptr<BackingStore>>;
  using MountMap = PathMap<struct EdenMountInfo, AbsolutePath>;
  class ThriftServerEventHandler;

  // Forbidden copy constructor and assignment operator
  EdenServer(EdenServer const&) = delete;
  EdenServer& operator=(EdenServer const&) = delete;

  void startPeriodicTasks();
  void updatePeriodicTaskIntervals(const EdenConfig& config);

  /**
   * Schedule a call to unloadInodes() to happen after timeout
   * has expired.
   * Must be called only from the mainEventBase_ thread.
   */
  void scheduleInodeUnload(std::chrono::milliseconds timeout);

  // Perform unloading of inodes based on their last access time
  // and then schedule another call to unloadInodes() to happen
  // at the next appropriate interval.  The unload attempt applies to
  // all mounts.
  void unloadInodes();

  FOLLY_NODISCARD folly::Future<folly::Unit> createThriftServer();

  void prepareThriftAddress() const;

  /**
   * prepareImpl() contains the bulk of the implementation of prepare()
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepareImpl(
      std::shared_ptr<StartupLogger> logger);
  FOLLY_NODISCARD std::vector<folly::Future<folly::Unit>> prepareMountsTakeover(
      std::shared_ptr<StartupLogger> logger,
      std::vector<TakeoverData::MountInfo>&& takeoverMounts);
  FOLLY_NODISCARD std::vector<folly::Future<folly::Unit>> prepareMounts(
      std::shared_ptr<StartupLogger> logger);
  static void incrementStartupMountFailures();

#ifndef _WIN32
  /**
   * recoverImpl() contains the bulk of the implementation of recover()
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> recoverImpl(TakeoverData&& data);
#endif // !_WIN32

  /**
   * Load and parse existing eden config.
   */
  std::shared_ptr<cpptoml::table> parseConfig();

  /**
   * Replace the config file with the given table.
   */
  void saveConfig(const cpptoml::table& root);

  /**
   * Open local storage engine for caching source control data.
   * Returns whether the config was modified.
   */
  bool createStorageEngine(cpptoml::table& config);
  void openStorageEngine(StartupLogger& logger);

  // Called when a mount has been unmounted and has stopped.
  void mountFinished(
      EdenMount* mountPoint,
      std::optional<TakeoverData::MountInfo> takeover);

  FOLLY_NODISCARD folly::Future<folly::Unit> performNormalShutdown();
  void shutdownPrivhelper();

  // Starts up a new mount for edenMount, starting up the thread
  // pool and initializing the dependent bits.
  FOLLY_NODISCARD folly::Future<folly::Unit> performFreshStart(
      std::shared_ptr<EdenMount> edenMount,
      bool readOnly);

  // Performs a takeover initialization for the provided mount, loading the
  // state from the old incarnation and starting up the thread pool.
  FOLLY_NODISCARD folly::Future<folly::Unit> performTakeoverStart(
      std::shared_ptr<EdenMount> edenMount,
      TakeoverData::MountInfo&& takeover);
  FOLLY_NODISCARD folly::Future<folly::Unit> completeTakeoverStart(
      std::shared_ptr<EdenMount> edenMount,
      TakeoverData::MountInfo&& takeover);

  // Add the mount point to mountPoints_.
  // This also makes sure we don't have this path mounted already.
  void addToMountPoints(std::shared_ptr<EdenMount> edenMount);

  /**
   * Look up an EdenMount by the path where it is mounted.
   *
   * This is similar to getMount(), but will also return mounts that are still
   * initializing.  It is the caller's responsibility to ensure they do not
   * perform any inode operations on the returned mount without first verifying
   * it is ready for access.
   */
  std::shared_ptr<EdenMount> getMountUnsafe(AbsolutePathPiece mountPath) const;

  // Registers (or removes) stats callbacks for edenMount.
  // These are here rather than in EdenMount because we need to
  // hold an owning reference to the mount to safely sample stats.
  void registerStats(std::shared_ptr<EdenMount> edenMount);
  void unregisterStats(EdenMount* edenMount);

  // Registers inode population reports callback with the notifier.
  void registerInodePopulationReportsCallback();
  void unregisterInodePopulationReportsCallback();

  // Report memory usage statistics to ServiceData.
  void reportMemoryStats();

  // Compute stats for the local store and perform garbage collection if
  // necessary
  void manageLocalStore();

  // some backing store may require periodic maintenance, specifically rust
  // datapack store needs to release file descriptor it holds every once in a
  // while.
  void refreshBackingStore();

  // Tree overlay needs periodically run checkpoint to flush its journal file.
  void manageOverlay();

  // Run a garbage collection cycle over the inodes hierarchy.
  void workingCopyGC();

  // Cancel all subscribers on all mounts so that we can tear
  // down the thrift server without blocking
  void shutdownSubscribers();

  /**
   * collect the values of the counter for each HgQueuedBackingStore
   * accessed by calling the getCounterFromStore function on the
   * each store
   */
  std::vector<size_t> collectHgQueuedBackingStoreCounters(
      std::function<size_t(const HgQueuedBackingStore&)> getCounterFromStore);

  /*
   * Member variables.
   *
   * Note that the declaration order below is important for initialization
   * and cleanup order.  edenDir_ is near the top so it will be destroyed
   * last, as it holds the process-wide lock for our on-disk state.
   * mountPoints_ are near the bottom, so they get destroyed before the
   * backingStores_ and localStore_.
   */

  const std::vector<std::string> originalCommandLine_;
  EdenStateDir edenDir_;
  std::shared_ptr<EdenServiceHandler> handler_;
  std::shared_ptr<apache::thrift::ThriftServer> server_;
  std::shared_ptr<ThriftServerEventHandler> serverEventHandler_;

  ActivityRecorderFactory activityRecorderFactory_;
  BackingStoreFactory* const backingStoreFactory_;

  std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<BackingStoreMap> backingStores_;
  const std::shared_ptr<BlobCache> blobCache_;
  std::shared_ptr<TreeCache> treeCache_;
  std::shared_ptr<ReloadableConfig> config_;

  std::shared_ptr<folly::Synchronized<MountMap>> mountPoints_;

#ifndef _WIN32
  /**
   * A server that waits on a new edenfs process to attempt
   * a graceful restart, taking over our running mount points.
   */
  std::unique_ptr<TakeoverServer> takeoverServer_;
#endif // !_WIN32

  /**
   * Information about whether the EdenServer is starting, running, or shutting
   * down, including whether it is performing a graceful restart.
   */
  struct RunStateData {
    RunState state{RunState::STARTING};
    folly::File takeoverThriftSocket;
    /**
     * In the case of a takeover shutdown, this will be fulfilled after the
     * TakeoverServer sends the TakeoverData to the TakeoverClient.
     *
     * If the takeover was successful, this returns std::nullopt. If a
     * TakeoverData object is returned, that means the takeover attempt failed
     * and the server should be resumed using the given TakeoverData.
     */
    folly::Future<std::optional<TakeoverData>> shutdownFuture =
        folly::Future<std::optional<TakeoverData>>::makeEmpty();
  };
  folly::Synchronized<RunStateData> runningState_;

  /**
   * The EventBase driving the main thread loop.
   *
   * This is used to drive the the thrift server and can also be used for
   * scheduling other asynchronous operations.
   *
   * This is set when the EdenServer is started and is never updated after
   * this, so we do not need synchronization when reading it.
   */
  folly::EventBase* mainEventBase_;

  /**
   * Common state shared by all of the EdenMount objects.
   */
  const std::shared_ptr<ServerState> serverState_;

  /**
   * Start time of edenfs daemon
   */
  const std::chrono::time_point<std::chrono::steady_clock> startTime_{
      std::chrono::steady_clock::now()};

  /**
   * Build package version
   */
  const std::string version_;

  /**
   * Remounting progress state.
   */
  struct ProgressState {
    std::string mountPath;
    std::string localDir;
    uint16_t fsckPercentComplete{0};
    bool mountFinished{false};
    bool fsckStarted{false};
    ProgressState(std::string&& mountPath, std::string&& localDir)
        : mountPath(std::move(mountPath)), localDir(std::move(localDir)) {}
  };

  /**
   * Manage remounting progress states.
   */
  struct ProgressManager {
    static constexpr size_t kMaxProgressLines = 8;
    std::vector<ProgressState> progresses;

    size_t totalLinesPrinted{0};
    size_t totalFinished{0};
    size_t totalInProgress{0};

    /**
     * Register a ProgressState for a mount point before remounting starts.
     */
    size_t registerEntry(std::string&& mountPath, std::string&& localDir);

    void updateProgressState(size_t processIndex, uint16_t percent);

    /**
     * Print registered progress states to stdout
     */
    void printProgresses(std::shared_ptr<StartupLogger>);

    /**
     * Update fsck completion percent and mark fsckStarted as true. Then refresh
     * the progress states printed to stdout. This function is triggered when
     * OverlayChecker calls back
     */
    void manageProgress(
        std::shared_ptr<StartupLogger> logger,
        size_t processIndex,
        uint16_t percent);

    /**
     * Mark mountFinished as true when a remounting progress is finished.
     */
    void finishProgress(size_t processIndex);
  };

  const std::unique_ptr<folly::Synchronized<ProgressManager>> progressManager_;

  PeriodicFnTask<&EdenServer::reloadConfig> reloadConfigTask_{
      this,
      "reload_config"};
  PeriodicFnTask<&EdenServer::checkLockValidity> checkValidityTask_{
      this,
      "check_lock_validity"};
  PeriodicFnTask<&EdenServer::reportMemoryStats> memoryStatsTask_{
      this,
      "mem_stats"};
  PeriodicFnTask<&EdenServer::manageLocalStore> localStoreTask_{
      this,
      "local_store"};
  PeriodicFnTask<&EdenServer::refreshBackingStore> backingStoreTask_{
      this,
      "backing_store"};
  PeriodicFnTask<&EdenServer::manageOverlay> overlayTask_{this, "overlay"};
  PeriodicFnTask<&EdenServer::workingCopyGC> gcTask_{this, "working_copy_gc"};
};
} // namespace facebook::eden
