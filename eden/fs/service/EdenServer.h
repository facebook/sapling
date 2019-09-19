/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once

#include <folly/Executor.h>
#include <folly/File.h>
#include <folly/Portability.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/ThreadLocal.h>
#include <folly/experimental/StringKeyedMap.h>
#include <folly/futures/SharedPromise.h>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/service/PeriodicTask.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/tracing/EdenStats.h"
#include "eden/fs/utils/PathFuncs.h"

#ifdef _WIN32
#include "eden/fs/win/mount/EdenMount.h" // @manual
#include "eden/fs/win/utils/Stub.h" // @manual
#include "eden/fs/win/utils/UserInfo.h" // @manual
#else
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/takeover/TakeoverData.h"
#endif

constexpr folly::StringPiece kPeriodicUnloadCounterKey{"PeriodicUnloadCounter"};

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

namespace facebook {
namespace eden {

class BackingStore;
class BlobCache;
class Dirstate;
class EdenServiceHandler;
class LocalStore;
class MountInfo;
class StartupLogger;
#ifndef _WIN32
class TakeoverServer;
#endif

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
  using DirstateMap = folly::StringKeyedMap<std::shared_ptr<Dirstate>>;

  EdenServer(
      std::vector<std::string> originalCommandLine,
      UserInfo userInfo,
      std::unique_ptr<PrivHelper> privHelper,
      std::shared_ptr<const EdenConfig> edenConfig);

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
   * successfully and accepting thrift connections.
   *
   * If waitForMountCompletion is true the returned future will also not
   * become ready until all configured mount points have been remounted.
   * If an error occurs remounting some mount points the Future will complete
   * with an exception, but the server will still continue to run.  Everything
   * will be running normally except for the mount points that failed to be
   * remounted.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepare(
      std::shared_ptr<StartupLogger> logger,
      bool waitForMountCompletion = true);

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
  void performCleanup();

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
      folly::StringPiece mountPath);

  /**
   * Unmount all mount points maintained by this server, and wait for them to
   * be completely unmounted.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> unmountAll();

  /**
   * Stop all mount points maintained by this server so that they can then be
   * transferred to a new edenfs process to perform a graceful restart.
   */
  FOLLY_NODISCARD folly::Future<TakeoverData> stopMountsForTakeover();

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
  std::shared_ptr<EdenMount> getMount(folly::StringPiece mountPath) const;

  /**
   * Look up an EdenMount by the path where it is mounted.
   *
   * This is similar to getMount(), but will also return mounts that are still
   * initializing.  It is the caller's responsibility to ensure they do not
   * perform any inode operations on the returned mount without first verifying
   * it is ready for access.
   */
  std::shared_ptr<EdenMount> getMountUnsafe(folly::StringPiece mountPath) const;

  std::shared_ptr<LocalStore> getLocalStore() const {
    return localStore_;
  }

  const std::shared_ptr<BlobCache>& getBlobCache() const {
    return blobCache_;
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
      folly::StringPiece type,
      folly::StringPiece name);

  AbsolutePathPiece getEdenDir() const {
    return edenDir_.getPath();
  }

  const std::shared_ptr<ServerState>& getServerState() const {
    return serverState_;
  }

  std::shared_ptr<EdenStats> getSharedStats() {
    return std::shared_ptr<EdenStats>(serverState_, getStats());
  }

  EdenStats* getStats() {
    return &serverState_->getStats();
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
   * Get the main thread's EventBase.
   *
   * Callers can use this for scheduling work to be run in the main thread.
   */
  folly::EventBase* getMainEventBase() const {
    return mainEventBase_;
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

  using BackingStoreKey = std::pair<std::string, std::string>;
  using BackingStoreMap =
      std::unordered_map<BackingStoreKey, std::shared_ptr<BackingStore>>;
  using MountMap = folly::StringKeyedMap<struct EdenMountInfo>;
  class ThriftServerEventHandler;

  // Forbidden copy constructor and assignment operator
  EdenServer(EdenServer const&) = delete;
  EdenServer& operator=(EdenServer const&) = delete;

  void startPeriodicTasks();
  void updatePeriodicTaskIntervals(const EdenConfig& config);

  /**
   * Schedule a call to unloadInodes() to happen after timeout
   * has expired.
   * Must be called only from the eventBase thread.
   */
  void scheduleInodeUnload(std::chrono::milliseconds timeout);

  // Perform unloading of inodes based on their last access time
  // and then schedule another call to unloadInodes() to happen
  // at the next appropriate interval.  The unload attempt applies to
  // all mounts.
  void unloadInodes();

  std::shared_ptr<BackingStore> createBackingStore(
      folly::StringPiece type,
      folly::StringPiece name);
  FOLLY_NODISCARD folly::Future<folly::Unit> createThriftServer();

  void prepareThriftAddress();

  /**
   * prepareImpl() contains the bulk of the implementation of prepare()
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepareImpl(
      std::shared_ptr<StartupLogger> logger,
      bool waitForMountCompletion);
  FOLLY_NODISCARD std::vector<folly::Future<folly::Unit>> prepareMountsTakeover(
      std::shared_ptr<StartupLogger> logger,
      std::vector<TakeoverData::MountInfo>&& takeoverMounts);
  FOLLY_NODISCARD std::vector<folly::Future<folly::Unit>> prepareMounts(
      std::shared_ptr<StartupLogger> logger);

  /**
   * Create config file if this the first time running the server, otherwise
   * parse existing config file.
   *
   */
  std::shared_ptr<cpptoml::table> parseConfig();

  /**
   * Create default config toml table
   */
  std::shared_ptr<cpptoml::table> getDefaultConfig();

  /**
   * Open local storage engine for caching source control data.
   */
  void openStorageEngine(
      std::shared_ptr<cpptoml::table>,
      std::shared_ptr<StartupLogger> logger);

  // Called when a mount has been unmounted and has stopped.
  void mountFinished(
      EdenMount* mountPoint,
      std::optional<TakeoverData::MountInfo> takeover);

  FOLLY_NODISCARD folly::Future<folly::Unit> performNormalShutdown();
  FOLLY_NODISCARD folly::Future<folly::Unit> performTakeoverShutdown(
      folly::File thriftSocket);
  void shutdownPrivhelper();

  // Starts up a new fuse mount for edenMount, starting up the thread
  // pool and initializing the fuse session
  FOLLY_NODISCARD folly::Future<folly::Unit> performFreshFuseStart(
      std::shared_ptr<EdenMount> edenMount);

  // Performs a takeover initialization for the provided fuse mount,
  // loading the state from the old incarnation and starting up the
  // thread pool.
  FOLLY_NODISCARD folly::Future<folly::Unit> performTakeoverFuseStart(
      std::shared_ptr<EdenMount> edenMount,
      TakeoverData::MountInfo&& takeover);
  FOLLY_NODISCARD folly::Future<folly::Unit> completeTakeoverFuseStart(
      std::shared_ptr<EdenMount> edenMount,
      TakeoverData::MountInfo&& takeover);

  // Add the mount point to mountPoints_.
  // This also makes sure we don't have this path mounted already.
  void addToMountPoints(std::shared_ptr<EdenMount> edenMount);

  // Registers (or removes) stats callbacks for edenMount.
  // These are here rather than in EdenMount because we need to
  // hold an owning reference to the mount to safely sample stats.
  void registerStats(std::shared_ptr<EdenMount> edenMount);
  void unregisterStats(EdenMount* edenMount);

  // Report memory usage statistics to ServiceData.
  void reportMemoryStats();

  // Compute stats for the local store and perform garbage collection if
  // necessary
  void manageLocalStore();

  // Cancel all subscribers on all mounts so that we can tear
  // down the thrift server without blocking
  void shutdownSubscribers();

  /*
   * Member variables.
   *
   * Note that the declaration order below is important for initialization
   * and cleanup order.  edenDir_ is near the top so it will be destroyed last,
   * as it holds the process-wide lock for our on-disk state.
   * mountPoints_ are near the bottom, so they get destroyed before the
   * backingStores_ and localStore_.
   */

  const std::vector<std::string> originalCommandLine_;
  EdenStateDir edenDir_;
  std::shared_ptr<EdenServiceHandler> handler_;
  std::shared_ptr<apache::thrift::ThriftServer> server_;
  std::shared_ptr<ThriftServerEventHandler> serverEventHandler_;

  std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<BackingStoreMap> backingStores_;
  const std::shared_ptr<BlobCache> blobCache_;

  folly::Synchronized<MountMap> mountPoints_;

#ifndef _WIN32
  /**
   * A server that waits on a new edenfs process to attempt
   * a graceful restart, taking over our running mount points.
   */
  std::unique_ptr<TakeoverServer> takeoverServer_;
  folly::Promise<TakeoverData> takeoverPromise_;

#endif // !_WIN32

  /**
   * Information about whether the EdenServer is starting, running, or shutting
   * down, including whether it is performing a graceful restart.
   */
  struct RunStateData {
    RunState state{RunState::STARTING};
    bool takeoverShutdown{false};
    folly::File takeoverThriftSocket;
  };
  folly::Synchronized<RunStateData> runningState_;

  /**
   * Common state shared by all of the EdenMount objects.
   */
  const std::shared_ptr<ServerState> serverState_;

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

  PeriodicFnTask<&EdenServer::reloadConfig> reloadConfigTask_{this,
                                                              "reload_config"};
  PeriodicFnTask<&EdenServer::flushStatsNow> flushStatsTask_{this,
                                                             "flush_stats"};
#ifndef _WIN32
  PeriodicFnTask<&EdenServer::reportMemoryStats> memoryStatsTask_{this,
                                                                  "mem_stats"};
#endif
  PeriodicFnTask<&EdenServer::manageLocalStore> localStoreTask_{this,
                                                                "local_store"};
};
} // namespace eden
} // namespace facebook
