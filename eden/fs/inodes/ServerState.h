/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>

#include "eden/fs/config/CachedParsedFileMonitor.h"
#include "eden/fs/model/git/GitIgnoreFileParser.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/RefPtr.h"
#include "eden/fs/utils/UserInfo.h"

// For ConfigReloadBehavior
#include "eden/fs/config/gen-cpp2/eden_config_types.h"

namespace folly {
class EventBase;
class Executor;
} // namespace folly

namespace facebook::eden {

class Clock;
class EdenConfig;
class EdenStats;
class FaultInjector;
class FsEventLogger;
class IHiveLogger;
class NfsServer;
class Notifier;
class PrivHelper;
class ProcessInfoCache;
class ReloadableConfig;
class StructuredLogger;
class TopLevelIgnores;
class UnboundedQueueExecutor;

using EdenStatsPtr = RefPtr<EdenStats>;

/**
 * ServerState is the testabl, dependency injection seam for the inode
 * layer. It includes some platform abstractions like Clock, loggers,
 * and configuration, and state shared across multiple mounts.
 *
 * This is normally owned by the main EdenServer object. However unit
 * tests also create ServerState objects without an
 * EdenServer. ServerState should not contain expensive-to-create
 * objects or they should be abstracted behind an interface so
 * appropriate fakes can be used in tests.
 */
class ServerState {
 public:
  ServerState(
      UserInfo userInfo,
      EdenStatsPtr edenStats,
      std::shared_ptr<PrivHelper> privHelper,
      std::shared_ptr<UnboundedQueueExecutor> threadPool,
      std::shared_ptr<folly::Executor> fsChannelThreadPool,
      std::shared_ptr<Clock> clock,
      std::shared_ptr<ProcessInfoCache> processInfoCache,
      std::shared_ptr<StructuredLogger> structuredLogger,
      std::shared_ptr<IHiveLogger> hiveLogger,
      std::shared_ptr<ReloadableConfig> reloadableConfig,
      const EdenConfig& initialConfig,
      folly::EventBase* mainEventBase,
      std::shared_ptr<Notifier> notifier,
      bool enableFaultInjection = false);
  ~ServerState();

  /**
   * Set the path to the server's thrift socket.
   *
   * This is called by EdenServer once it has initialized the thrift server.
   */
  void setSocketPath(AbsolutePathPiece path) {
    socketPath_ = path.copy();
  }

  /**
   * Get the path to the server's thrift socket.
   *
   * This is used by the EdenMount to populate the `.eden/socket` special file.
   */
  const AbsolutePath& getSocketPath() const {
    return socketPath_;
  }

  /**
   * Get the EdenStats object that tracks process-wide (rather than per-mount)
   * statistics.
   */
  const EdenStatsPtr& getStats() const {
    return edenStats_;
  }

  const std::shared_ptr<ReloadableConfig>& getReloadableConfig() const {
    return config_;
  }

  /**
   * Get the EdenConfig data.
   *
   * The config data may be reloaded from disk depending on the value of the
   * reload parameter.
   */
  std::shared_ptr<const EdenConfig> getEdenConfig(
      ConfigReloadBehavior reload = ConfigReloadBehavior::AutoReload);

  /**
   * Get the TopLevelIgnores. It is based on the system and user git ignore
   * files.
   */
  std::unique_ptr<TopLevelIgnores> getTopLevelIgnores();

  /**
   * Get the UserInfo object describing the user running this edenfs process.
   */
  const UserInfo& getUserInfo() const {
    return userInfo_;
  }

  /**
   * Get the PrivHelper object used to perform operations that require
   * elevated privileges.
   */
  PrivHelper* getPrivHelper() {
    return privHelper_.get();
  }

  /**
   * Get the thread pool.
   *
   * Adding new tasks to this thread pool executor will never block.
   */
  const std::shared_ptr<UnboundedQueueExecutor>& getThreadPool() const {
    return threadPool_;
  }

  /**
   * Get the FS channel thread pool.
   *
   * FS channel requests are intended to run on this thread pool.
   */
  const std::shared_ptr<folly::Executor>& getFsChannelThreadPool() const {
    return fsChannelThreadPool_;
  }

  /**
   * Get the Clock.
   */
  const std::shared_ptr<Clock>& getClock() const {
    return clock_;
  }

  const std::shared_ptr<NfsServer>& getNfsServer() const& {
    return nfs_;
  }

  const std::shared_ptr<ProcessInfoCache>& getProcessInfoCache() const {
    return processInfoCache_;
  }

  const std::shared_ptr<StructuredLogger>& getStructuredLogger() const {
    return structuredLogger_;
  }

  /**
   * Returns a HiveLogger that can be used to send log events to external
   * long term storage for offline consumption. Prefer this method if the
   * caller needs to own a reference due to lifetime mismatch with the
   * ServerState
   */
  const std::shared_ptr<IHiveLogger>& getHiveLogger() const {
    return hiveLogger_;
  }

  /**
   * Returns a pointer to the FsEventLogger for logging FS event samples, if the
   * platform supports it. Otherwise, returns nullptr. The caller is responsible
   * for null checking.
   */
  const std::shared_ptr<FsEventLogger>& getFsEventLogger() const {
    return fsEventLogger_;
  }

  FaultInjector& getFaultInjector() {
    return *faultInjector_;
  }

  const std::shared_ptr<Notifier>& getNotifier() {
    return notifier_;
  }

 private:
  AbsolutePath socketPath_;
  UserInfo userInfo_;
  EdenStatsPtr edenStats_;
  std::shared_ptr<PrivHelper> privHelper_;
  std::shared_ptr<UnboundedQueueExecutor> threadPool_;
  std::shared_ptr<folly::Executor> fsChannelThreadPool_;
  std::shared_ptr<Clock> clock_;
  std::shared_ptr<ProcessInfoCache> processInfoCache_;
  std::shared_ptr<StructuredLogger> structuredLogger_;
  std::shared_ptr<IHiveLogger> hiveLogger_;
  std::unique_ptr<FaultInjector> const faultInjector_;
  std::shared_ptr<NfsServer> nfs_;

  std::shared_ptr<ReloadableConfig> config_;
  folly::Synchronized<CachedParsedFileMonitor<GitIgnoreFileParser>>
      userIgnoreFileMonitor_;
  folly::Synchronized<CachedParsedFileMonitor<GitIgnoreFileParser>>
      systemIgnoreFileMonitor_;
  std::shared_ptr<Notifier> notifier_;
  std::shared_ptr<FsEventLogger> fsEventLogger_;
};
} // namespace facebook::eden
