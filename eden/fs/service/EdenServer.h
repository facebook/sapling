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
#include <string>
#include <unordered_map>
#include <vector>
#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/fuse/EdenStats.h"
#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/utils/PathFuncs.h"
#include "folly/experimental/FunctionScheduler.h"

constexpr folly::StringPiece kPeriodicUnloadCounterKey{"PeriodicUnloadCounter"};
constexpr folly::StringPiece kPrivateBytes{"memory_private_bytes"};
constexpr folly::StringPiece kRssBytes{"memory_vm_rss_bytes"};
constexpr std::chrono::seconds kMemoryPollSeconds{30};

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
class Dirstate;
class EdenServiceHandler;
class LocalStore;
class MountInfo;
class StartupLogger;
class TakeoverServer;

/*
 * EdenServer contains logic for running the Eden main loop.
 *
 * It performs locking to ensure only a single EdenServer instance is running
 * for a particular location, then starts the thrift management server
 * and the fuse session.
 */
class EdenServer : private TakeoverHandler {
 public:
  using MountList = std::vector<std::shared_ptr<EdenMount>>;
  using DirstateMap = folly::StringKeyedMap<std::shared_ptr<Dirstate>>;

  EdenServer(
      UserInfo userInfo,
      std::unique_ptr<PrivHelper> privHelper,
      std::unique_ptr<const EdenConfig> edenConfig);

  virtual ~EdenServer();

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
   * The returned future does not complete until all configured mount points
   * have been remounted and until the thrift server is accepting connections.
   * If an error occurs remounting some mount points the Future will complete
   * with an exception, but the server will still continue to run.  Everything
   * will be running normally except for the mount points that failed to be
   * remounted.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepare(
      std::shared_ptr<StartupLogger> logger);

  /**
   * Run the EdenServer.
   *
   * prepare() must have been called before calling run(), but the future
   * returned by prepare() does not need to be complete yet.
   */
  void run();

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
      std::unique_ptr<ClientConfig> initialConfig,
      folly::Optional<TakeoverData::MountInfo>&& optionalTakeover =
          folly::none);

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

  MountList getMountPoints() const;

  /**
   * Look up an EdenMount by the path where it is mounted.
   *
   * Throws an EdenError if no mount exists with the specified path.
   */
  std::shared_ptr<EdenMount> getMount(folly::StringPiece mountPath) const;

  /**
   * Look up an EdenMount by the path where it is mounted.
   *
   * Returns nullptr if no mount exists with the specified path.
   */
  std::shared_ptr<EdenMount> getMountOrNull(folly::StringPiece mountPath) const;

  std::shared_ptr<LocalStore> getLocalStore() const {
    return localStore_;
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

  AbsolutePathPiece getEdenDir() {
    return edenDir_;
  }

  std::shared_ptr<const EdenConfig> getEdenConfig() {
    return *edenConfig_.rlock();
  }

  const std::shared_ptr<ServerState>& getServerState() {
    return serverState_;
  }
  ThreadLocalEdenStats* getStats() {
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
   * Report Linux specific statistics.  They are computed by parsing
   * files in the proc file system (eg. /proc/self/smaps).
   */
  void reportProcStats();

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
    folly::Optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;

    explicit EdenMountInfo(const std::shared_ptr<EdenMount>& mount)
        : edenMount(mount),
          unmountPromise(folly::SharedPromise<folly::Unit>()) {}
  };

  using BackingStoreKey = std::pair<std::string, std::string>;
  using BackingStoreMap =
      std::unordered_map<BackingStoreKey, std::shared_ptr<BackingStore>>;
  using MountMap = folly::StringKeyedMap<struct EdenMountInfo>;
  class ThriftServerEventHandler;

  // Forbidden copy constructor and assignment operator
  EdenServer(EdenServer const&) = delete;
  EdenServer& operator=(EdenServer const&) = delete;

  // Schedules a timer to flush stats (and reschedule itself).
  // We should have at most one of these pending at a time.
  // Must be called only from the eventBase thread.
  void scheduleFlushStats();

  // Schedule a call to unloadInodes() to happen after timeout
  // has expired.
  // Must be called only from the eventBase thread.
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

  /**
   * Acquire the main edenfs lock.
   *
   * Returns true if the lock was acquired successfully, or false if we failed
   * to acquire the lock (likely due to another process holding it).
   * May throw an exception on other errors (e.g., insufficient permissions to
   * create the lock file, out of disk space, etc).
   */
  FOLLY_NODISCARD bool acquireEdenLock();

  void prepareThriftAddress();

  /**
   * prepareImpl() contains the bulk of the implementation of prepare()
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> prepareImpl(
      std::shared_ptr<StartupLogger> logger);

  // Called when a mount has been unmounted and has stopped.
  void mountFinished(
      EdenMount* mountPoint,
      folly::Optional<TakeoverData::MountInfo> takeover);

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

  // Cancel all subscribers on all mounts so that we can tear
  // down the thrift server without blocking
  void shutdownSubscribers();

  /*
   * Member variables.
   *
   * Note that the declaration order below is important for initialization
   * and cleanup order.  lockFile_ is near the top so it will be released last.
   * mountPoints_ are near the bottom, so they get destroyed before the
   * backingStores_ and localStore_.
   */

  AbsolutePath edenDir_;
  AbsolutePath configPath_;
  folly::File lockFile_;
  std::shared_ptr<EdenServiceHandler> handler_;
  std::shared_ptr<apache::thrift::ThriftServer> server_;
  std::shared_ptr<ThriftServerEventHandler> serverEventHandler_;

  std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<BackingStoreMap> backingStores_;

  folly::Synchronized<MountMap> mountPoints_;

  /**
   * A server that waits on a new edenfs process to attempt
   * a graceful restart, taking over our running mount points.
   */
  std::unique_ptr<TakeoverServer> takeoverServer_;
  folly::Promise<TakeoverData> takeoverPromise_;

  enum class RunState {
    STARTING,
    RUNNING,
    SHUTTING_DOWN,
  };
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
   * EdenConfig
   */
  folly::Synchronized<std::shared_ptr<const EdenConfig>> edenConfig_;

  /**
   * Common state shared by all of the EdenMount objects.
   */
  std::shared_ptr<ServerState> serverState_;

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
   * Track the last time we calculated the /proc based statistics.
   * We use this for throttling purposes. Note: we use time since epoch since
   * std::atomic chokes on time_point.
   */
  std::atomic<std::chrono::system_clock::duration> lastProcStatsRun_;
};
} // namespace eden
} // namespace facebook
