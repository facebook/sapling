/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#include <cpptoml.h>
#include <algorithm>
#include <chrono>

#include <sys/stat.h>
#include <fstream>
#include <functional>
#include <iterator>
#include <memory>
#include <sstream>
#include <string>

#include "eden/fs/service/HeartbeatManager.h"

#include <fb303/ServiceData.h>
#include <fmt/core.h>
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>

#include <folly/json/json.h>
#ifdef __APPLE__
#include <folly/Subprocess.h> // @manual
#endif
#include <folly/chrono/Conv.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/executors/thread_factory/NamedThreadFactory.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/io/async/HHWheelTimer.h>
#include <folly/logging/xlog.h>
#include <folly/portability/SysTypes.h>
#include <folly/stop_watch.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>
#include <thrift/lib/cpp2/async/ServerPublisherStream.h>
#include <thrift/lib/cpp2/async/ServerStream.h>
#include <thrift/lib/cpp2/server/ParallelConcurrencyController.h>
#include <thrift/lib/cpp2/server/RoundRobinRequestPile.h>
#include <thrift/lib/cpp2/server/ThriftProcessor.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include <csignal>

#include "eden/common/telemetry/RequestMetricsScope.h"
#include "eden/common/telemetry/SessionInfo.h"
#include "eden/common/telemetry/StructuredLoggerFactory.h"
#include "eden/common/utils/EnumValue.h"
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/FileUtils.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/ProcessInfoCache.h"
#include "eden/common/utils/TimeUtil.h"
#include "eden/common/utils/UnboundedQueueExecutor.h"
#include "eden/common/utils/UserInfo.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/config/TomlConfig.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeAccessLogger.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/ServerState.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/nfs/NfsServer.h"
#include "eden/fs/notifications/NullNotifier.h"
#include "eden/fs/privhelper/PrivHelper.h"
#include "eden/fs/service/EdenCPUThreadPool.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/service/StartupStatusSubscriber.h"
#include "eden/fs/service/ThriftStreamStartupStatusSubscriber.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/UsageService.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/SqliteLocalStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/store/hg/SaplingBackingStore.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/EdenStructuredLogger.h"
#include "eden/fs/telemetry/IScribeLogger.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/EdenTaskQueue.h"
#include "eden/fs/utils/FsChannelTypes.h"
#include "eden/fs/utils/NfsSocket.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/ProcUtil.h"

#ifdef EDEN_HAVE_USAGE_SERVICE
#include "eden/fs/service/facebook/EdenFSSmartPlatformServiceEndpoint.h" // @manual
#endif

#ifdef EDEN_HAVE_SERVER_OBSERVER
#include "common/fb303/cpp/ThreadPoolExecutorCounters.h" // @manual
#include "common/network/Hostname.h"
#include "eden/fs/service/facebook/ServerObserver.h" // @manual
#endif

#ifndef _WIN32
#include <sys/wait.h>
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/notifications/CommandNotifier.h"
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverServer.h"
#endif

#ifdef _WIN32
#include "eden/fs/notifications/WindowsNotifier.h" // @manual
#endif // !_WIN32

DEFINE_bool(
    debug,
    false,
    "run fuse in debug mode"); // TODO: remove; no longer needed
DEFINE_bool(
    takeover,
    false,
    "If another edenfs process is already running, "
    "attempt to gracefully takeover its mount points.");
DEFINE_bool(
    enable_fault_injection,
    false,
    "Enable the fault injection framework.");

#define DEFAULT_STORAGE_ENGINE "rocksdb"
#define SUPPORTED_STORAGE_ENGINES "rocksdb|sqlite|memory"

DEFINE_string(
    local_storage_engine_unsafe,
    "",
    "Select storage engine. " DEFAULT_STORAGE_ENGINE
    " is the default. "
    "possible choices are (" SUPPORTED_STORAGE_ENGINES
    "). "
    "memory is currently very dangerous as you will "
    "lose state across restarts and graceful restarts! "
    "This flag will only be used on the first invocation");

DEFINE_int64(
    start_delay_minutes,
    10,
    "Initial delay before first background inode unload");
DEFINE_int64(
    unload_age_minutes,
    6 * 60,
    "Minimum age of the inodes to be unloaded in background");

using apache::thrift::ThriftServer;
using folly::Future;
using folly::makeFuture;
using folly::makeFutureWith;
using folly::StringPiece;
using folly::Unit;
using std::make_shared;
using std::optional;
using std::shared_ptr;
using std::string;
using namespace std::chrono_literals;

namespace {

using namespace facebook::eden;

std::shared_ptr<Notifier> getPlatformNotifier(
    std::shared_ptr<ReloadableConfig> config,
    std::shared_ptr<StructuredLogger> logger,
    std::string version) {
#if defined(_WIN32)
  /*
   * If the E-Menu is disabled, we should create a Null Notifier
   * that no-ops when EdenFS attempts to send notifications
   * through it.
   */
  if (config->getEdenConfig()->enableEdenMenu.getValue()) {
    /*
     * The startTime we're passing will be slightly different than the actual
     * start time... However, this doesn't matter too much. We will already be
     * showing a slightly incorrect uptime because the E-Menu won't update the
     * uptime until the user re-clicks on the "About EdenFS" menu option
     */
    try {
      auto notifier = std::make_shared<WindowsNotifier>(
          config, logger, version, std::chrono::steady_clock::now());
      notifier->initialize();
      return notifier;
    } catch (const std::exception& ex) {
      auto reason = folly::exceptionStr(ex);
      XLOGF(WARN, "Couldn't start E-Menu: {}", reason);
      logger->logEvent(EMenuStartupFailure{reason.toStdString()});
    }
  }
  return std::make_shared<NullNotifier>(config);
#else
  (void)version;
  (void)logger;
  return std::make_shared<CommandNotifier>(config);
#endif // _WIN32
}

constexpr StringPiece kRocksDBPath{"storage/rocks-db"};
constexpr StringPiece kSqlitePath{"storage/sqlite.db"};
constexpr StringPiece kSlStorePrefix{"store.sapling"};
#ifndef _WIN32
constexpr StringPiece kFuseRequestPrefix{"fuse"};
#endif
#ifdef __APPLE__
constexpr StringPiece kNFSStatPrefix{"nfs"};
#endif
constexpr StringPiece kStateConfig{"config.toml"};

std::optional<std::string> getUnixDomainSocketPath(
    const folly::SocketAddress& address) {
  return AF_UNIX == address.getFamily() ? std::make_optional(address.getPath())
                                        : std::nullopt;
}

#ifdef __APPLE__
folly::Try<folly::dynamic> collectNFSUtilStats() {
  try {
    SpawnedProcess::Options opts;
    opts.pipeStdout();
    opts.pipeStderr();
    auto nfsStatProc =
        SpawnedProcess({"nfsstat", "-f", "JSON"}, std::move(opts));
    std::string nfsStatOutputString = nfsStatProc.communicate().first;
    nfsStatProc.waitTimeout(1s);
    return folly::Try<folly::dynamic>(folly::parseJson(nfsStatOutputString));
  } catch (const std::exception& e) {
    return folly::Try<folly::dynamic>(e);
  }
}
#endif

std::string getCounterNameForImportMetric(
    RequestMetricsScope::RequestStage stage,
    RequestMetricsScope::RequestMetric metric,
    std::optional<SaplingBackingStore::SaplingImportObject> object =
        std::nullopt) {
  if (object.has_value()) {
    // base prefix . stage . object . metric
    return folly::to<std::string>(
        kSlStorePrefix,
        ".",
        RequestMetricsScope::stringOfHgImportStage(stage),
        ".",
        SaplingBackingStore::stringOfSaplingImportObject(object.value()),
        ".",
        RequestMetricsScope::stringOfRequestMetric(metric));
  }
  // base prefix . stage . metric
  return folly::to<std::string>(
      kSlStorePrefix,
      ".",
      RequestMetricsScope::stringOfHgImportStage(stage),
      ".",
      RequestMetricsScope::stringOfRequestMetric(metric));
}

#ifndef _WIN32
std::string getCounterNameForFuseRequests(
    RequestMetricsScope::RequestStage stage,
    RequestMetricsScope::RequestMetric metric,
    const EdenMount* mount) {
  auto mountName = basename(mount->getPath().view());
  // prefix . mount . stage . metric
  return folly::to<std::string>(
      kFuseRequestPrefix,
      ".",
      mountName,
      ".",
      RequestMetricsScope::stringOfFuseRequestStage(stage),
      ".",
      RequestMetricsScope::stringOfRequestMetric(metric));
}
#endif

#ifdef __linux__
// **not safe to call this function from a fuse thread**
// this gets the kernels view of the number of pending requests, to do this, it
// stats the fuse mount root in the filesystem which could call into the FUSE
// daemon which could cause a deadlock.
size_t getNumberPendingFuseRequests(const EdenMount* mount) {
  constexpr StringPiece kFuseInfoDir{"/sys/fs/fuse/connections"};
  constexpr StringPiece kFusePendingRequestFile{"waiting"};

  auto mount_path = mount->getPath().c_str();
  struct stat file_metadata;

  folly::checkUnixError(
      lstat(mount_path, &file_metadata),
      "unable to get FUSE device number for mount ",
      basename(mount->getPath().view()));

  auto pending_request_path = folly::to<std::string>(
      kFuseInfoDir,
      kDirSeparator,
      file_metadata.st_dev,
      kDirSeparator,
      kFusePendingRequestFile);

  auto pending_requests = readFile(canonicalPath(pending_request_path));
  return pending_requests.hasValue()
      ? folly::to<size_t>(pending_requests.value())
      : 0;
}
#endif // __linux__

std::shared_ptr<folly::Executor> makeCheckoutRevisionThreads(
    bool useCheckoutExecutor,
    std::shared_ptr<const EdenConfig>& edenConfig) {
  if (useCheckoutExecutor) {
    return std::make_shared<UnboundedQueueExecutor>(
        edenConfig->numCheckoutThreads.getValue(),
        "CheckoutRevisionThreadPool");
  }
  return nullptr;
}

} // namespace

namespace facebook::eden {

class EdenServer::ThriftServerEventHandler
    : public apache::thrift::server::TServerEventHandler,
      public folly::AsyncSignalHandler {
 public:
  explicit ThriftServerEventHandler(EdenServer* edenServer)
      : AsyncSignalHandler{nullptr},
        edenServer_{edenServer},
        stoppingFromSignal_{false} {}

  void preServe(const folly::SocketAddress* address) override {
    if (edenServer_->getServerState()
            ->getEdenConfig()
            ->thriftUseCustomPermissionChecking.getValue()) {
      if (auto path = getUnixDomainSocketPath(*address)) {
        folly::checkUnixError(
            chmod(path->c_str(), 0777), "failed to chmod ", *path, " to 777");
      }
    }

    // preServe() will be called from the thrift server thread once when it is
    // about to start serving.
    //
    // Register for SIGINT and SIGTERM.  We do this in preServe() so we can use
    // the thrift server's EventBase to process the signal callbacks.
    auto eventBase = folly::EventBaseManager::get()->getEventBase();
    attachEventBase(eventBase);
    registerSignalHandler(SIGINT);
    registerSignalHandler(SIGTERM);
#ifndef _WIN32
    registerSignalHandler(SIGCHLD);
#endif
    runningPromise_.setValue();
  }

  void signalReceived(int sig) noexcept override {
    switch (sig) {
#ifndef _WIN32
      case SIGCHLD:
        XLOG(DBG4, "got SIGCHLD");
        {
          // Clean up zombie processes (ex. `sl debugrefreshconfig`).
          int status;
          pid_t pid;
          while ((pid = waitpid(-1, &status, WNOHANG)) > 0) {
            XLOGF(DBG5, "waited pid {} status {}", pid, status);
          }
        }
        return;
#endif
      case SIGINT:
      case SIGTERM:
        if (stoppingFromSignal_) {
          XLOGF(INFO, "already stopping due to signal {}", sig);
        } else {
          stoppingFromSignal_ = true;

          XLOGF(INFO, "stopping due to signal {}", sig);

          auto shutdownTimeout = edenServer_->getServerState()
                                     ->getEdenConfig()
                                     ->sigtermShutdownTimeout.getValue();

          if (sig == SIGINT || shutdownTimeout.count() == 0) {
            // Unregister signal handler if either:
            //   - sig is SIGINT (this way, the process exits immediately when
            //   you mash ctrl-c).
            //   - shutdownTimeout is disabled.
            //
            // Unregistering the signal handler causes us to exit immediately
            // via default signal handling the next time we get the signal.
            XLOGF(INFO, "unregistering signal handler for signal {}", sig);
            unregisterSignalHandler(sig);
          } else {
            // Otherwise we are using shutdownTimeout. Schedule a call to
            // _exit() after the timeout.
            XLOGF(
                INFO,
                "scheduling _exit() if not done shutting down in {}",
                durationToString(shutdownTimeout));
            edenServer_->getMainEventBase()->runAfterDelay(
                [sig, shutdownTimeout] {
                  XLOGF(
                      INFO,
                      "_exit()ing after {} timeout on signal {}",
                      durationToString(shutdownTimeout),
                      sig);
                  folly::LoggerDB::get().flushAllHandlers();
                  _exit(1);
                },
                std::chrono::duration_cast<std::chrono::milliseconds>(
                    shutdownTimeout)
                    .count());
          }

#ifndef _WIN32
          // Record the signal that caused daemon to
          // stop. The next edenFS startup record this
          // signal in the silent daemon exit logger
          edenServer_->createDaemonExitSignalFile(sig);
#endif
          // Stop the server.
          edenServer_->stop();
        }
    }
  }

  /**
   * Return a Future that will be fulfilled once the thrift server is bound to
   * its socket and is ready to accept connections.
   */
  folly::SemiFuture<Unit> getThriftRunningFuture() {
    return runningPromise_.getSemiFuture();
  }

 private:
  EdenServer* edenServer_{nullptr};
  folly::Promise<Unit> runningPromise_;
  bool stoppingFromSignal_;
};

static constexpr folly::StringPiece kNfsReadCount60{"nfs.read_us.count.60"};
static constexpr folly::StringPiece kNfsReadDirCount60{
    "nfs.readdir_us.count.60"};
static constexpr folly::StringPiece kNfsReadDirPlusCount60{
    "nfs.readdirplus_us.count.60"};

static constexpr folly::StringPiece kFsChannelTaskCount{"fs.task.count"};
static constexpr folly::StringPiece kMemoryVmRssBytes{"memory_vm_rss_bytes"};
#ifdef __APPLE__
static constexpr folly::StringPiece kMemoryCompressedBytes{
    "memory_compressed_bytes"};
static constexpr folly::StringPiece kMemoryFootprintBytes{
    "memory_footprint_bytes"};
#endif

EdenServer::EdenServer(
    std::vector<std::string> originalCommandLine,
    UserInfo userInfo,
    EdenStatsPtr edenStats,
    SessionInfo sessionInfo,
    std::unique_ptr<PrivHelper> privHelper,
    std::shared_ptr<const EdenConfig> edenConfig,
    ActivityRecorderFactory activityRecorderFactory,
    BackingStoreFactory* backingStoreFactory,
    std::shared_ptr<IScribeLogger> scribeLogger,
    std::shared_ptr<StartupStatusChannel> startupStatusChannel,
    std::string version)
    : originalCommandLine_{std::move(originalCommandLine)},
      edenDir_{edenConfig->edenDir.getValue()},
      activityRecorderFactory_{std::move(activityRecorderFactory)},
      backingStoreFactory_{backingStoreFactory},
      config_{std::make_shared<ReloadableConfig>(edenConfig)},
      mountPoints_{std::make_shared<folly::Synchronized<MountMap>>(
          MountMap{kPathMapDefaultCaseSensitive})},
      // Store a pointer to the EventBase that will be used to drive
      // the main thread.  The runServer() code will end up driving this
      // EventBase.
      mainEventBase_{folly::EventBaseManager::get()->getEventBase()},
      structuredLogger_{
          makeDefaultStructuredLogger<EdenStructuredLogger, EdenStatsPtr>(
              edenConfig->scribeLogger.getValue(),
              edenConfig->scribeCategory.getValue(),
              sessionInfo,
              edenStats.copy())},
      notificationsStructuredLogger_{
          makeDefaultStructuredLogger<EdenStructuredLogger, EdenStatsPtr>(
              edenConfig->scribeLogger.getValue(),
              edenConfig->notificationsScribeCategory.getValue(),
              sessionInfo,
              edenStats.copy())},
      heartbeatManager_{
          std::make_unique<HeartbeatManager>(edenDir_, structuredLogger_)},
      serverState_{make_shared<ServerState>(
          std::move(userInfo),
          std::move(edenStats),
          std::move(sessionInfo),
          std::move(privHelper),
          std::make_shared<EdenCPUThreadPool>(
              edenConfig->edenCpuPoolNumThreads.getValue()),
          std::make_shared<UnboundedQueueExecutor>(
              edenConfig->numFsChannelThreads.getValue(),
              "FsChannelThreadPool"),
          std::make_shared<UnixClock>(),
          std::make_shared<ProcessInfoCache>(),
          structuredLogger_,
          notificationsStructuredLogger_,
          std::move(scribeLogger),
          config_,
          *edenConfig,
          mainEventBase_,
          getPlatformNotifier(config_, structuredLogger_, version),
          FLAGS_enable_fault_injection)},
      blobCache_{BlobCache::create(
          serverState_->getReloadableConfig(),
          serverState_->getStats().copy())},
      treeCache_{TreeCache::create(
          serverState_->getReloadableConfig(),
          serverState_->getStats().copy())},
      version_{std::move(version)},
      thriftUseResourcePools_{edenConfig->thriftUseResourcePools.getValue()},
      thriftUseSerialExecution_{
          edenConfig->thriftUseSerialExecution.getValue()},
      thriftUseCheckoutExecutor_{
          edenConfig->thriftUseCheckoutExecutor.getValue()},
      checkoutRevisionExecutor_{
          makeCheckoutRevisionThreads(thriftUseCheckoutExecutor_, edenConfig)},
      progressManager_{
          std::make_unique<folly::Synchronized<EdenServer::ProgressManager>>()},
      startupStatusChannel_{std::move(startupStatusChannel)} {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();

  registerInodePopulationReportsCallback();

  for (auto stage : RequestMetricsScope::requestStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : SaplingBackingStore::saplingImportObjects) {
        auto counterName = getCounterNameForImportMetric(stage, metric, object);
        counters->registerCallback(counterName, [this, stage, object, metric] {
          auto individual_counters = this->collectSaplingBackingStoreCounters(
              [stage, object, metric](const SaplingBackingStore& store) {
                return store.getImportMetric(stage, object, metric);
              });
          return RequestMetricsScope::aggregateMetricCounters(
              metric, individual_counters);
        });
      }
      auto summaryCounterName = getCounterNameForImportMetric(stage, metric);
      counters->registerCallback(summaryCounterName, [this, stage, metric] {
        std::vector<size_t> individual_counters;
        for (auto object : SaplingBackingStore::saplingImportObjects) {
          auto more_counters = this->collectSaplingBackingStoreCounters(
              [stage, object, metric](const SaplingBackingStore& store) {
                return store.getImportMetric(stage, object, metric);
              });
          individual_counters.insert(
              individual_counters.end(),
              more_counters.begin(),
              more_counters.end());
        }
        return RequestMetricsScope::aggregateMetricCounters(
            metric, individual_counters);
      });
    }
  }

  counters->registerCallback(kFsChannelTaskCount, [this] {
    auto fsChannelExecutor = this->getServerState()->getFsChannelThreadPool();
    if (auto ex = std::dynamic_pointer_cast<folly::CPUThreadPoolExecutor>(
            fsChannelExecutor)) {
      return ex->getTaskQueueSize();
    }
    if (auto ex = std::dynamic_pointer_cast<UnboundedQueueExecutor>(
            fsChannelExecutor)) {
      return ex->getTaskQueueSize();
    }
    return (size_t)0;
  });

  counters->registerCallback(kMemoryVmRssBytes, [] {
    auto memoryStats = facebook::eden::proc_util::readMemoryStats();
    if (memoryStats) {
      return memoryStats->resident;
    } else {
      return (size_t)0;
    }
  });

#ifdef __APPLE__
  // On macOS, export extra memory counters
  counters->registerCallback(kMemoryCompressedBytes, [] {
    auto memoryStats = facebook::eden::proc_util::readMemoryStats();
    if (memoryStats && memoryStats->compressed) {
      return memoryStats->compressed.value();
    } else {
      return (size_t)0;
    }
  });

  counters->registerCallback(kMemoryFootprintBytes, [] {
    auto memoryStats = facebook::eden::proc_util::readMemoryStats();
    if (memoryStats && memoryStats->footprint) {
      return memoryStats->footprint.value();
    } else {
      return (size_t)0;
    }
  });

  // On macOS, export the NFS clients/servers counters
  if (config_->getEdenConfig()->updateNFSStatsInterval.getValue() > 0ms) {
    auto result = collectNFSUtilStats();
    if (result.hasValue()) {
      for (const auto& nfsStatsCounter : kNfsStatsToEdenStatsMap_) {
        auto counterName = mapCounterNameForNFSStat(nfsStatsCounter);
        if (counterName.has_value()) {
          counters->registerCallback(
              counterName.value(), [this, nfsStatsCounter] {
                auto result =
                    this->getNFSStatCounterValue(nfsStatsCounter.first);
                if (result.has_value()) {
                  return result.value();
                } else {
                  return 0LL;
                }
              });
        } else {
          // This is not an error, just log it and continue.
          // This only happen on registration time, not during runtime per
          // counter. It notify us that we have a NFS counter in the map that
          // is not reported by Apple.
          XLOGF(
              DFATAL,
              "macOS doesn't report any stat for: {}",
              nfsStatsCounter.first);
        }
      }
    } else {
      auto error = result.exception();
      // This is not a fatal error, just log it and continue.
      // This only happen on registration time, not during runtime per
      // counter.
      XLOGF(
          ERR,
          "Failed to collect NFS clients/servers counters: {}",
          error.what());
    }
  }
#endif
}

EdenServer::~EdenServer() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();

  unregisterInodePopulationReportsCallback();

  for (auto stage : RequestMetricsScope::requestStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : SaplingBackingStore::saplingImportObjects) {
        auto counterName = getCounterNameForImportMetric(stage, metric, object);
        counters->unregisterCallback(counterName);
      }
      auto summaryCounterName = getCounterNameForImportMetric(stage, metric);
      counters->unregisterCallback(summaryCounterName);
    }
  }
  counters->unregisterCallback(kFsChannelTaskCount);
#ifdef __APPLE__
  counters->unregisterCallback(kMemoryCompressedBytes);
  counters->unregisterCallback(kMemoryFootprintBytes);

  for (const auto& nfsStatsCounter : kNfsStatsToEdenStatsMap_) {
    auto counterName = mapCounterNameForNFSStat(nfsStatsCounter);
    if (counterName.has_value()) {
      counters->unregisterCallback(counterName.value());
    }
  }
#endif
}

#ifdef __APPLE__
std::optional<std::string> EdenServer::mapCounterNameForNFSStat(
    std::pair<std::string, std::string> nfsStatsCounter) {
  auto result = this->getNFSStatCounterValue(nfsStatsCounter.first);
  if (result.has_value()) {
    return fmt::format(
        fmt::runtime(kNFSStatPrefix.str() + ".{}"),
        nfsStatsCounter.second); // e.g. "nfs.requests"
  } else {
    return std::nullopt;
  }
}

std::optional<long long> EdenServer::getNFSStatCounterValue(
    std::string nfsStatsCounterMacOSName) {
  if (!this->updateNFSStatsIfNeeded()) {
    // Unable to collect NFS stats
    return std::nullopt;
  }
  try {
    std::vector<std::string> tokens;
    folly::split('.', nfsStatsCounterMacOSName, tokens);
    return nfsStatOutput_[tokens[0]][tokens[1]][tokens[2]].asInt();
  } catch (const std::exception&) {
    return std::nullopt;
  }
}

bool EdenServer::updateNFSStatsIfNeeded() {
  auto now = std::chrono::steady_clock::now();
  auto last = lastTimeUpdatedNfsStat_.wlock();
  if (now >=
      *last + config_->getEdenConfig()->updateNFSStatsInterval.getValue()) {
    auto result = collectNFSUtilStats();
    *last = std::chrono::steady_clock::now();
    if (!result.hasValue()) {
      // Unable to collect NFS stats
      return false;
    }
    nfsStatOutput_ = result.value();
  }
  return true;
}
#endif

namespace cursor_helper {

// https://vt100.net/docs/vt510-rm/CPL.html
// The cursor is moved to the start of the nth preceding line
std::string move_cursor_up(size_t n) {
  return fmt::format("\x1b\x5b{}F", n);
}

// https://vt100.net/docs/vt510-rm/ED.html
// Erases characters from the cursor through to the end of the display
std::string clear_to_bottom() {
  return "\x1b\x5bJ";
}
} // namespace cursor_helper

void EdenServer::ProgressManager::updateProgressState(
    size_t progressIndex,
    uint16_t percent) {
  if (progressIndex < totalInProgress) {
    progresses[progressIndex].fsckPercentComplete = percent;
    progresses[progressIndex].fsckStarted = true;
  }
}

void EdenServer::ProgressManager::finishProgress(size_t progressIndex) {
  progresses[progressIndex].mountFinished = true;

  totalFinished++;
  totalInProgress--;
}

void EdenServer::ProgressManager::markFailed(size_t progressIndex) {
  progresses[progressIndex].mountFailed = true;

  totalFailed++;
  totalInProgress--;
}

void EdenServer::ProgressManager::printProgresses(
    std::shared_ptr<StartupLogger> logger,
    std::optional<std::string_view> errorMessage) {
  std::string prepare;
  std::string content;

  if (totalLinesPrinted) {
    prepare = cursor_helper::move_cursor_up(totalLinesPrinted);
    totalLinesPrinted = 0;
  }
  prepare += cursor_helper::clear_to_bottom();

  // we intentionally don't include the lines here in totalLinesPrinted so that
  // they won't be erased next time.
  if (errorMessage.has_value()) {
    content += errorMessage.value();
  }

  size_t printedFinished = 0;
  size_t printedFailed = 0;
  size_t printedInProgress = 0;
  for (auto& it : progresses) {
    if (it.mountFinished) {
      content += fmt::format("Successfully remounted {}\n", it.mountPath);
      printedFinished++;
    } else if (it.mountFailed) {
      content += fmt::format("Failed to remount {}\n", it.mountPath);
      printedFailed++;
    } else if (!it.fsckStarted) {
      content += fmt::format("Remounting {}\n", it.mountPath);
      printedInProgress++;
    } else {
      content += fmt::format(
          "[{:21}] {:>3}%: fsck on {}{}",
          std::string(it.fsckPercentComplete * 2, '=') + ">",
          it.fsckPercentComplete * 10,
          it.localDir,
          "\n");
      printedInProgress++;
    }
    totalLinesPrinted++;
    if (totalLinesPrinted == kMaxProgressLines) {
      content += fmt::format(
          "and {} finished, {} failed, {} in progress...",
          totalFinished - printedFinished,
          totalFailed - printedFailed,
          totalInProgress - printedInProgress);
      break;
    }
  }
  logger->logVerbose(prepare + content);
  totalLinesPrinted++;
}

void EdenServer::ProgressManager::manageProgress(
    std::shared_ptr<StartupLogger> logger,
    size_t progressIndex,
    uint16_t percent) {
  updateProgressState(progressIndex, percent);
  printProgresses(logger);
}
size_t EdenServer::ProgressManager::registerEntry(
    std::string&& mountPath,
    std::string&& localDir) {
  auto progressIndex = progresses.size();
  progresses.emplace_back(std::move(mountPath), std::move(localDir));
  totalInProgress++;
  return progressIndex;
}

folly::SemiFuture<Unit> EdenServer::unmountAll() {
  std::vector<folly::SemiFuture<Unit>> futures;
  {
    const auto mountPoints = mountPoints_->wlock();
    for (auto& entry : *mountPoints) {
      auto& info = entry.second;

      // Note: capturing the shared_ptr<EdenMount> here in the thenTry() lambda
      // is important to ensure that the EdenMount object cannot be destroyed
      // before EdenMount::unmount() completes.
      auto mount = info.edenMount;
      auto future = mount->unmount({}).defer(
          [mount, unmountFuture = info.unmountPromise.getFuture()](
              auto&& result) mutable {
            if (result.hasValue()) {
              return std::move(unmountFuture);
            } else {
              XLOGF(
                  ERR,
                  "Failed to perform unmount for \"{}\": {}",
                  mount->getPath(),
                  result.exception().what());
              return makeFuture<Unit>(result.exception());
            }
          });
      futures.push_back(std::move(future));
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).deferValue(
      [](std::vector<folly::Try<Unit>> results) {
        for (const auto& result : results) {
          result.throwUnlessValue();
        }
      });
}

#ifndef _WIN32
Future<TakeoverData> EdenServer::stopMountsForTakeover(
    folly::Promise<std::optional<TakeoverData>>&& takeoverPromise) {
  std::vector<Future<optional<TakeoverData::MountInfo>>> futures;
  {
    const auto mountPoints = mountPoints_->wlock();
    for (auto& [mountPath, info] : *mountPoints) {
      try {
        info.takeoverPromise.emplace();
        auto future = info.takeoverPromise->getFuture();

        FsChannel* fsChannel = info.edenMount->getFsChannel();
        if (!fsChannel) {
          return EDEN_BUG_FUTURE(TakeoverData)
              << "Takeover isn't (yet) supported during mount initialization."
              << "Mount State "
              << folly::to_underlying(info.edenMount->getState());
        }

        XLOGF(DBG7, "Calling takeoverStop on {} channel", fsChannel->getName());
        if (fsChannel->takeoverStop()) {
          // Success! Takeover has begun.
        } else {
          return EDEN_BUG_FUTURE(TakeoverData)
              << "Takeover isn't (yet) supported for " << fsChannel->getName()
              << " mounts. Mount state: "
              << folly::to_underlying(info.edenMount->getState());
        }

        futures.emplace_back(std::move(future).thenValue(
            [self = this,
             edenMount = info.edenMount](TakeoverData::MountInfo takeover)
                -> Future<optional<TakeoverData::MountInfo>> {
              auto fuseChannelInfo =
                  std::get_if<FuseChannelData>(&takeover.channelInfo);
              auto nfsChannelInfo =
                  std::get_if<NfsChannelData>(&takeover.channelInfo);
              if (!fuseChannelInfo && !nfsChannelInfo) {
                return std::nullopt;
              }
              auto& fd = fuseChannelInfo ? fuseChannelInfo->fd
                                         : nfsChannelInfo->nfsdSocketFd;
              if (!fd) {
                return std::nullopt;
              }
              return self->serverState_->getPrivHelper()
                  ->takeoverShutdown(edenMount->getPath().view())
                  .thenValue([takeover = std::move(takeover)](auto&&) mutable {
                    return std::move(takeover);
                  });
            }));
      } catch (...) {
        auto ew = folly::exception_wrapper{std::current_exception()};
        XLOGF(
            ERR, "Error while stopping \"{}\" for takeover: {}", mountPath, ew);
        futures.push_back(
            makeFuture<optional<TakeoverData::MountInfo>>(std::move(ew)));
      }
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  // We should not be using .via(&InlineExecutor::instance()) here, this is
  // unsafe and deadlock prone. See eden/fs/docs/Futures.md for more details.
  return folly::collectAll(futures)
      .via(&folly::InlineExecutor::instance())
      .thenValue([takeoverPromise = std::move(takeoverPromise)](
                     std::vector<folly::Try<optional<TakeoverData::MountInfo>>>
                         results) mutable {
        TakeoverData data;
        data.takeoverComplete = std::move(takeoverPromise);
        data.mountPoints.reserve(results.size());
        for (auto& result : results) {
          // If something went wrong shutting down a mount point,
          // log the error but continue trying to perform graceful takeover
          // of the other mount points.
          if (!result.hasValue()) {
            // TODO: Log this type of error either in the new process or the old
            // process.
            XLOGF(
                ERR,
                "error stopping mount during takeover shutdown: {}",
                result.exception().what());
            continue;
          }

          // result might be a successful Try with an empty Optional.
          // This could happen if the mount point was unmounted while we were
          // in the middle of stopping it for takeover.  Just skip this mount
          // in this case.
          if (!result.value().has_value()) {
            // TODO: Log this type of error either in the new process or the old
            // process.
            XLOG(WARN, "mount point was unmounted during takeover shutdown");
            continue;
          }

          data.mountPoints.emplace_back(std::move(result.value().value()));
        }
        return data;
      });
}
#endif

void EdenServer::startPeriodicTasks() {
  auto config = serverState_->getReloadableConfig()->getEdenConfig();
  if (config->enableOBCOnEden.getValue()) {
    // Get the hostname without the ".facebook.com" suffix
    auto hostname = facebook::network::getLocalHost(/*stripFbDomain=*/true);
    std::vector<std::string> entities;
    auto reWorkerID = std::getenv("REMOTE_EXECUTION_WORKER");
    if (reWorkerID == nullptr) {
      entities = {hostname};
    } else {
      entities = {hostname, fmt::format("{}:{}", hostname, reWorkerID)};
    }
    memory_vm_rss_bytes_ = monitoring::OBCAvg(
        monitoring::OdsCategoryId::ODS_EDEN,
        fmt::format("eden.{}", kMemoryVmRssBytes),
        entities);
    // Report memory usage stats once every 60 seconds
    memoryStatsTask_.updateInterval(60s);
  }
  updatePeriodicTaskIntervals(*config);

#ifndef _WIN32
  // Schedule a periodic job to unload unused inodes based on the last access
  // time. currently Eden does not have accurate timestamp tracking for inodes,
  // so using unloadChildrenNow just to validate the behaviour. We will have to
  // modify current unloadChildrenNow function to unload inodes based on the
  // last access time.
  if (config->periodicUnloadIntervalMinutes.getValue() > 0) {
    scheduleInodeUnload(std::chrono::minutes(FLAGS_start_delay_minutes));
  }
#endif

  backingStoreTask_.updateInterval(1min);
}

void EdenServer::updatePeriodicTaskIntervals(const EdenConfig& config) {
  // Update all periodic tasks whose interval is
  // controlled by EdenConfig settings.

  reloadConfigTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.configReloadInterval.getValue()));

  // The checkValidityTask_ isn't really needed on Windows, since the lock file
  // cannot be removed while we are holding it.
#ifndef _WIN32
  checkValidityTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.checkValidityInterval.getValue()));
#endif

  overlayTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.overlayMaintenanceInterval.getValue()));

  localStoreTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.localStoreManagementInterval.getValue()));
  /**
   * For now, periodic GC only makes sense on Windows and macOS, with unknown
   * behavior on Linux.
   */
  if (config.enableGc.getValue()) {
    gcTask_.updateInterval(
        std::chrono::duration_cast<std::chrono::milliseconds>(
            config.gcPeriod.getValue()));
  }

  if (config.enableNfsServer.getValue() &&
      config.enableNfsCrawlDetection.getValue()) {
    auto interval = config.nfsCrawlInterval.getValue();
    XLOGF(
        DBG4,
        "NFS crawl detection enabled. Using interval = {}ns",
        interval.count());
    detectNfsCrawlTask_.updateInterval(
        std::chrono::duration_cast<std::chrono::milliseconds>(interval));
  } else {
    detectNfsCrawlTask_.updateInterval(0s);
  }

  accidentalUnmountRecoveryTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.accidentalUnmountRecoveryInterval.getValue()));

#ifndef _WIN32
  updateEdenHeartbeatFileTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.updateEdenHeartbeatFileInterval.getValue()));
#endif
}

void EdenServer::scheduleCallbackOnMainEventBase(
    std::chrono::milliseconds timeout,
    std::function<void()> fn) {
  // If we don't care about calling fn_ when the callback is canceled, we
  // could just use scheduleTimeoutFn and not have to do a wrapper class.
  // We don't want to run fn_ on cancel because we could be running in the
  // middle of destruction.
  struct Wrapper : folly::HHWheelTimer::Callback {
    explicit Wrapper(std::function<void()> f) : fn_(std::move(f)) {}
    void timeoutExpired() noexcept override {
      // Delete this even under exceptions.
      std::unique_ptr<Wrapper> self{this};

      XLOG(DBG3, "Callback expired, running function");
      try {
        fn_();
      } catch (std::exception const& e) {
        XLOG(
            ERR,
            "HHWheelTimerBase timeout callback threw an exception: {}",
            e.what());
      } catch (...) {
        XLOG(ERR, "HHWheelTimerBase timeout callback threw a non-exception.");
      }
    }
    // The callback will be canceled if the timer is destroyed. Or we use
    // deduplication We don't want to run fn during destruction.
    void callbackCanceled() noexcept override {
      // Delete this even under exceptions.
      std::unique_ptr<Wrapper> self{this};
      XLOG(DBG3, "Callback cancelled, NOT running function");
    }
    std::function<void()> fn_;
  };

  mainEventBase_->runInEventBaseThread(
      [timeout, evb = mainEventBase_, fn = std::move(fn)]() mutable {
        // I hate using raw pointers, but we have to to schedule the callback.
        Wrapper* w = new Wrapper(std::move(fn));
        // Note callback will be run in the mainEventBase_ Thread.
        // TODO: would be nice to deduplicate these function calls. We need the
        // same type of callback to use the same wrapper object ... singletons?
        // The main difference with scheduleTimeoutFn is whether the callback is
        // invoked when the EventBase is destroyed: scheduleTimeoutFn will call
        // it, this function won't. See comment above the wrapper class.
        evb->timer().scheduleTimeout(w, timeout);
      });
}

size_t EdenServer::enumerateInProgressCheckouts() {
  size_t numActive = 0;
  auto mountPoints = mountPoints_->rlock();
  for (auto& entry : *mountPoints) {
    auto& info = entry.second;
    numActive += info.edenMount->isCheckoutInProgress() ? 1 : 0;
  }
  return numActive;
}

#ifndef _WIN32
void EdenServer::unloadInodes() {
  auto mounts = getMountPoints();

  if (!mounts.empty()) {
    auto cutoff = std::chrono::system_clock::now() -
        std::chrono::minutes(FLAGS_unload_age_minutes);
    auto cutoff_ts = folly::to<timespec>(cutoff);
    for (auto& mountHandle : mounts) {
      auto& mount = mountHandle.getEdenMount();
      auto& rootInode = mountHandle.getRootInode();
      auto unloaded = rootInode->unloadChildrenLastAccessedBefore(cutoff_ts);
      if (unloaded) {
        XLOGF(
            DBG6,
            "Unloaded {} inodes in background from mount {}",
            unloaded,
            mount.getPath());
      }
      mount.getInodeMap()->recordPeriodicInodeUnload(unloaded);
    }
  }
  scheduleInodeUnload(
      std::chrono::minutes(serverState_->getReloadableConfig()
                               ->getEdenConfig()
                               ->periodicUnloadIntervalMinutes.getValue()));
}

void EdenServer::scheduleInodeUnload(std::chrono::milliseconds timeout) {
  mainEventBase_->timer().scheduleTimeoutFn(
      [this] {
        XLOG(DBG4, "Beginning periodic inode unload");
        unloadInodes();
      },
      timeout);
}

#endif // !_WIN32

ImmediateFuture<Unit> EdenServer::recover(TakeoverData&& data) {
  return recoverImpl(std::move(data))
      .ensure(
          // Mark the server state as RUNNING once we finish setting up the
          // mount points. Even if an error occurs we still transition to the
          // running state.
          [this] {
            auto state = runningState_.wlock();
            state->shutdownFuture =
                folly::Future<std::optional<TakeoverData>>::makeEmpty();
            state->state = RunState::RUNNING;
          });
}

ImmediateFuture<Unit> EdenServer::recoverImpl(TakeoverData&& takeoverData) {
  auto thriftRunningFuture = createThriftServer();

  const auto takeoverPath = edenDir_.getTakeoverSocketPath();

  // Recover the eden lock file and the thrift server socket.
  auto oldDaemonPid = edenDir_.takeoverLock(std::move(takeoverData.lockFile));
  if (oldDaemonPid.has_value()) {
    oldDaemonPid_ = oldDaemonPid.value();
  }
  server_->useExistingSocket(takeoverData.thriftSocket.release());

  // Remount our mounts from our prepared takeoverData
  std::vector<ImmediateFuture<Unit>> mountFutures;
  mountFutures = prepareMountsTakeover(
      std::make_unique<ForegroundStartupLogger>(startupStatusChannel_),
      std::move(takeoverData.mountPoints));

  // Return a future that will complete only when all mount points have
  // started and the thrift server is also running.
  mountFutures.emplace_back(std::move(thriftRunningFuture));
  return collectAll(std::move(mountFutures)).unit();
}

void EdenServer::serve() const {
  getServer()->serve();
}

std::string EdenServer::getEdenHeartbeatFileNameStr() const {
  return heartbeatManager_->getHeartbeatFileName();
}

std::optional<std::string> EdenServer::getOldEdenHeartbeatFileNameStr() const {
  if (oldDaemonPid_.has_value()) {
    return edenDir_.getHeartbeatFileNamePrefix().toString() +
        oldDaemonPid_.value();
  } else {
    return std::nullopt;
  }
}

#ifndef _WIN32
void EdenServer::createOrUpdateEdenHeartbeatFile() {
  heartbeatManager_->createOrUpdateHeartbeatFile();
}

void EdenServer::createDaemonExitSignalFile(int signal) {
  heartbeatManager_->createDaemonExitSignalFile(signal);
}

void EdenServer::removeEdenHeartbeatFile() const {
  heartbeatManager_->removeHeartbeatFile();
}

bool EdenServer::checkForPreviousHeartbeat(bool takeover) {
  return heartbeatManager_->checkForPreviousHeartbeat(
      takeover, getOldEdenHeartbeatFileNameStr());
}
#endif

Future<Unit> EdenServer::prepare(std::shared_ptr<StartupLogger> logger) {
  return prepareImpl(std::move(logger))
      .ensure(
          // Mark the server state as RUNNING once we finish setting up the
          // mount points. Even if an error occurs we still transition to the
          // running state. The prepare() code will log an error with more
          // details if we do fail to set up some of the mount points.
          [this] { runningState_.wlock()->state = RunState::RUNNING; });
}

namespace {
bool shouldThrowDuringTakeover(const EdenConfig& config) {
  return config.clientThrowsDuringTakeover.getValue();
}

std::chrono::seconds getTakeoverTimeoutSeconds(const EdenConfig& config) {
  return std::chrono::duration_cast<std::chrono::seconds>(
      config.takeoverReceiveTimeout.getValue());
}
} // namespace

Future<Unit> EdenServer::prepareImpl(std::shared_ptr<StartupLogger> logger) {
  bool doingTakeover = false;
  auto edenDirLockResult = edenDir_.acquireLock();
  if (edenDirLockResult.second.has_value()) {
    // If it is during graceful restart and the old edenfs daemon process is
    // still running, we need to keep the old daemon pid.
    oldDaemonPid_ = edenDirLockResult.second;
  }
  if (!edenDirLockResult.first) {
    // Another edenfs process is already running.
    //
    // If --takeover was specified, fall through and attempt to gracefully
    // takeover mount points from the existing daemon.
    //
    // If --takeover was not specified, fail now.
    if (!FLAGS_takeover) {
      throwf<std::runtime_error>(
          "another instance of Eden appears to be running for {}",
          edenDir_.getPath());
    }
    doingTakeover = true;
  }
  auto thriftRunningFuture = createThriftServer();
  // Start the PrivHelper client, using our main event base to drive its I/O
  serverState_->getPrivHelper()->attachEventBase(mainEventBase_);

  startPeriodicTasks();

#ifndef _WIN32
  // If we are gracefully taking over from an existing edenfs process,
  // receive its lock, thrift socket, and mount points now.
  // This will shut down the old process.
  const auto takeoverPath = edenDir_.getTakeoverSocketPath();
  TakeoverData takeoverData;
#endif
  if (doingTakeover) {
#ifndef _WIN32
    logger->log(
        "Requesting existing edenfs process to gracefully "
        "transfer its mount points...");
    auto edenConfig = serverState_->getEdenConfig();
    takeoverData = takeoverMounts(
        takeoverPath,
        getTakeoverTimeoutSeconds(*edenConfig),
        shouldThrowDuringTakeover(*edenConfig));
    logger->log(
        "Received takeover information for ",
        takeoverData.mountPoints.size(),
        " mount points");

    // Take over the eden lock file and the thrift server socket.
    auto oldDaemonPid = edenDir_.takeoverLock(std::move(takeoverData.lockFile));
    if (oldDaemonPid.has_value()) {
      oldDaemonPid_ = oldDaemonPid.value();
    }
    server_->useExistingSocket(takeoverData.thriftSocket.release());
#else
    NOT_IMPLEMENTED();
#endif // !_WIN32
  } else {
    // Remove any old thrift socket from a previous (now dead) edenfs daemon.
    prepareThriftAddress();
  }

  if (auto nfsServer = serverState_->getNfsServer()) {
#ifndef _WIN32
    if (doingTakeover && takeoverData.mountdServerSocket.has_value()) {
      XLOG(DBG7, "Initializing mountd from existing socket");
      nfsServer->initialize(std::move(takeoverData.mountdServerSocket.value()));
    } else {
#endif
      XLOG(DBG7, "Initializing mountd from scratch");
      std::optional<AbsolutePath> unixSocketPath;
      if (serverState_->getEdenConfig()->useUnixSocket.getValue()) {
        unixSocketPath = edenDir_.getMountdSocketPath();
      }
      nfsServer->initialize(
          makeNfsSocket(std::move(unixSocketPath)),
          serverState_->getEdenConfig()->registerMountd.getValue());
#ifndef _WIN32
    }
#endif
  }

  // TODO: The "state config" only has one configuration knob now. When
  // another is required, introduce an EdenStateConfig class to manage
  // defaults and save on update.
  auto config = parseConfig();
  bool shouldSaveConfig = createStorageEngine(*config);
  if (shouldSaveConfig) {
    saveConfig(*config);
  }

#ifndef _WIN32
  // Start listening for graceful takeover requests
  takeoverServer_.reset(new TakeoverServer(
      getMainEventBase(),
      takeoverPath,
      this,
      &serverState_->getFaultInjector()));
  takeoverServer_->start();
#endif // !_WIN32

  return via(
      mainEventBase_,
      [this,
       logger,
       doingTakeover,
#ifndef _WIN32
       takeoverData = std::move(takeoverData),
#endif
       thriftRunningFuture = std::move(thriftRunningFuture)]() mutable {
        try {
          // it's important that this be the first thing that happens in this
          // future. The local store needs to be setup before mounts can be
          // accessed but also errors here may cause eden to bail out early. We
          // don't want eden to bail out with partially setup mounts.
          openStorageEngine(*logger);
        } catch (const std::exception& ex) {
          throw LocalStoreOpenError(ex.what());
        }
        std::vector<ImmediateFuture<Unit>> mountFutures;
        if (doingTakeover) {
#ifndef _WIN32
          mountFutures = prepareMountsTakeover(
              logger, std::move(takeoverData.mountPoints));
#else
          NOT_IMPLEMENTED();
#endif // !_WIN32
        } else {
          mountFutures = prepareMounts(logger);
        }

        // Return a future that will complete only when all mount points have
        // started and the thrift server is also running.
        mountFutures.emplace_back(std::move(thriftRunningFuture));
        return collectAll(std::move(mountFutures)).unit().semi();
      });
}

std::shared_ptr<cpptoml::table> EdenServer::parseConfig() {
  auto configPath = edenDir_.getPath() + RelativePathPiece{kStateConfig};

  std::ifstream inputFile(configPath.c_str());
  if (!inputFile.is_open()) {
    if (errno != ENOENT) {
      folly::throwSystemErrorExplicit(
          errno, "unable to open EdenFS config ", configPath.view());
    }
    // No config file, assume an empty table.
    return cpptoml::make_table();
  }
  return cpptoml::parser(inputFile).parse();
}

void EdenServer::saveConfig(const cpptoml::table& root) {
  auto configPath = edenDir_.getPath() + RelativePathPiece{kStateConfig};

  std::ostringstream stream;
  stream << root;

  writeFileAtomic(configPath, folly::StringPiece(stream.str())).value();
}

bool EdenServer::createStorageEngine(cpptoml::table& config) {
  std::string defaultStorageEngine = FLAGS_local_storage_engine_unsafe.empty()
      ? DEFAULT_STORAGE_ENGINE
      : FLAGS_local_storage_engine_unsafe;
  auto [storageEngine, configUpdated] =
      setDefault(config, {"local-store", "engine"}, defaultStorageEngine);

  if (!FLAGS_local_storage_engine_unsafe.empty() &&
      FLAGS_local_storage_engine_unsafe != storageEngine) {
    throw std::runtime_error(folly::to<string>(
        "--local_storage_engine_unsafe flag ",
        FLAGS_local_storage_engine_unsafe,
        "does not match last recorded flag ",
        storageEngine));
  }

  if (storageEngine == "memory") {
    XLOG(DBG2, "Creating new memory store.");
    localStore_ = make_shared<MemoryLocalStore>(getStats().copy());
  } else if (storageEngine == "sqlite") {
    const auto path = edenDir_.getPath() + RelativePathPiece{kSqlitePath};
    const auto parentDir = path.dirname();
    ensureDirectoryExists(parentDir);
    XLOGF(DBG2, "Creating local SQLite store {}...", path);
    folly::stop_watch<std::chrono::milliseconds> watch;
    localStore_ = make_shared<SqliteLocalStore>(path, getStats().copy());
    XLOGF(
        DBG2,
        "Opened SQLite store in {:.1f} seconds.",
        watch.elapsed().count() / 1000.0);
  } else if (storageEngine == "rocksdb") {
    XLOG(DBG2, "Creating local RocksDB store...");
    folly::stop_watch<std::chrono::milliseconds> watch;
    const auto rocksPath = edenDir_.getPath() + RelativePathPiece{kRocksDBPath};
    ensureDirectoryExists(rocksPath);
    localStore_ = make_shared<RocksDbLocalStore>(
        rocksPath,
        getStats().copy(),
        serverState_->getStructuredLogger(),
        &serverState_->getFaultInjector(),
        config_);
    XLOGF(
        DBG2,
        "Created RocksDB store in {} seconds.",
        watch.elapsed().count() / 1000.0);
  } else {
    throw std::runtime_error(
        folly::to<string>("invalid storage engine: ", storageEngine));
  }

  return configUpdated;
}

void EdenServer::openStorageEngine(StartupLogger& logger) {
  logger.log("Opening local store...");
  folly::stop_watch<std::chrono::milliseconds> watch;
  serverState_->getFaultInjector().check("open_local_store");
  localStore_->open();
  logger.log(
      "Opened local store in ", watch.elapsed().count() / 1000.0, " seconds.");
}

std::vector<ImmediateFuture<Unit>> EdenServer::prepareMountsTakeover(
    shared_ptr<StartupLogger> logger,
    std::vector<TakeoverData::MountInfo>&& takeoverMounts) {
  // Trigger remounting of existing mount points
  // If doingTakeover is true, use the mounts received in TakeoverData
  std::vector<ImmediateFuture<Unit>> mountFutures;

  if (folly::kIsWindows) {
    NOT_IMPLEMENTED();
  }

  for (auto& info : takeoverMounts) {
    const auto stateDirectory = info.stateDirectory;
    auto mountFuture =
        makeImmediateFutureWith([&] {
          auto initialConfig = CheckoutConfig::loadFromClientDirectory(
              AbsolutePathPiece{info.mountPath},
              AbsolutePathPiece{info.stateDirectory});

          return mount(
              std::move(initialConfig), false, [](auto) {}, std::move(info));
        })
            .thenTry(
                [logger, mountPath = info.mountPath](
                    folly::Try<std::shared_ptr<EdenMount>>&& result)
                    -> ImmediateFuture<folly::Unit> {
                  if (result.hasValue()) {
                    logger->log("Successfully took over mount ", mountPath);
                    return folly::unit;
                  } else {
                    incrementStartupMountFailures();
                    logger->warn(
                        "Failed to perform takeover for ",
                        mountPath,
                        ": ",
                        result.exception().what());
                    return makeImmediateFuture<Unit>(
                        std::move(result).exception());
                  }
                });
    mountFutures.push_back(std::move(mountFuture));
  }

  return mountFutures;
}

std::vector<ImmediateFuture<Unit>> EdenServer::prepareMounts(
    shared_ptr<StartupLogger> logger) {
  std::vector<ImmediateFuture<Unit>> mountFutures;
  folly::dynamic dirs = folly::dynamic::object();
  try {
    dirs = CheckoutConfig::loadClientDirectoryMap(edenDir_.getPath());
  } catch (...) {
    auto ew = folly::exception_wrapper{std::current_exception()};
    incrementStartupMountFailures();
    logger->warn(
        "Could not parse config.json file: ",
        ew.what(),
        "\nSkipping remount step.");
    mountFutures.emplace_back(std::move(ew));
    return mountFutures;
  }

  if (dirs.empty()) {
    logger->log("No mount points currently configured.");
    return mountFutures;
  }
  logger->log("Remounting ", dirs.size(), " mount points...");

  for (const auto& client : dirs.items()) {
    auto mountFuture = makeImmediateFutureWith([&] {
      auto mountPath = canonicalPath(client.first.stringPiece());
      auto edenClientPath =
          edenDir_.getCheckoutStateDir(client.second.stringPiece());
      auto initialConfig =
          CheckoutConfig::loadFromClientDirectory(mountPath, edenClientPath);
      auto progressIndex = progressManager_->wlock()->registerEntry(
          client.first.asString(), initialConfig->getOverlayPath().c_str());

      return mount(
                 std::move(initialConfig),
                 false,
                 [this, logger, progressIndex](auto percent) {
                   progressManager_->wlock()->manageProgress(
                       logger, progressIndex, percent);
                 })
          .thenTry([this, logger, mountPath, progressIndex](
                       folly::Try<std::shared_ptr<EdenMount>>&& result) {
            if (result.hasValue()) {
              auto wl = progressManager_->wlock();
              wl->finishProgress(progressIndex);
              wl->printProgresses(logger);
              return ImmediateFuture<folly::Unit>{std::in_place};
            } else {
              incrementStartupMountFailures();
              auto errorMessage = fmt::format(
                  "Failed to remount {}: {}\n",
                  mountPath.value(),
                  result.exception().what());
              XLOG(DBG7, errorMessage);
              auto wl = progressManager_->wlock();
              wl->markFailed(progressIndex);
              wl->printProgresses(logger, errorMessage);
              return makeImmediateFuture<Unit>(std::move(result).exception());
            }
          });
    });

    mountFutures.push_back(std::move(mountFuture));
  }
  return mountFutures;
}

void EdenServer::incrementStartupMountFailures() {
  // Increment a counter to track if there were any errors remounting checkouts
  // during startup.
  fb303::fbData->incrementCounter("startup_mount_failures");
}

void EdenServer::closeStorage() {
  // Destroy the local store and backing stores.
  // We shouldn't access the local store any more after giving up our
  // lock, and we need to close it to release its lock before the new
  // edenfs process tries to open it.
  backingStores_.wlock()->clear();

  // Explicitly close the LocalStore
  // Since we have a shared_ptr to it, other parts of the code can
  // theoretically still maintain a reference to it after the EdenServer is
  // destroyed. We want to ensure that it is really closed and no subsequent
  // I/O can happen to it after the EdenServer is shut down and the main Eden
  // lock is released.
  localStore_->close();
}

bool EdenServer::performCleanup() {
  bool takeover = false;

  folly::stop_watch<> shutdown;
  bool shutdownSuccess = true;
  SCOPE_EXIT {
    auto shutdownTimeInSeconds =
        std::chrono::duration<double>{shutdown.elapsed()}.count();
    serverState_->getStructuredLogger()->logEvent(
        DaemonStop{shutdownTimeInSeconds, takeover, shutdownSuccess});
  };

  auto shutdownFuture =
      folly::SemiFuture<std::optional<TakeoverData>>::makeEmpty();
  {
    auto state = runningState_.wlock();
    takeover = state->shutdownFuture.valid();
    if (takeover) {
      shutdownFuture = std::move(state->shutdownFuture);
    }
    XDCHECK_EQ(state->state, RunState::SHUTTING_DOWN);
    state->state = RunState::SHUTTING_DOWN;
  }

#ifdef EDEN_HAVE_SERVER_OBSERVER
  // Stop the server observer publish thread. If this is a takeover and the
  // takeover fails, the recovery code will restart the thread again
  fb303::ThreadCachedServiceData::get()->stopPublishThread();
#endif

  if (!takeover) {
    shutdownFuture = performNormalShutdown().deferValue(
        [](auto&&) -> std::optional<TakeoverData> { return std::nullopt; });
  }

  XCHECK(shutdownFuture.valid())
      << "shutdownFuture should not be empty during cleanup";

  // Drive the main event base until shutdownFuture completes
  XCHECK_EQ(mainEventBase_, folly::EventBaseManager::get()->getEventBase());
  auto shutdownResult =
      std::move(shutdownFuture).via(mainEventBase_).getTryVia(mainEventBase_);

#ifndef _WIN32
  shutdownSuccess = !shutdownResult.hasException();

  // We must check if the shutdownResult contains TakeoverData, and if so
  // we must recover
  if (shutdownResult.hasValue()) {
    auto&& shutdownValue = shutdownResult.value();
    if (shutdownValue.has_value()) {
      // shutdownValue only contains a value if a takeover was not successful.
      shutdownSuccess = false;
      XLOG(INFO, "edenfs encountered a takeover error, attempting to recover");
      // We do not wait here for the remounts to succeed, and instead will
      // let runServer() drive the mainEventBase loop to finish this call
      folly::futures::detachOn(
          getServerState()->getThreadPool().get(),
          recover(std::move(shutdownValue).value()).semi());
      return false;
    }
  }
#endif

  closeStorage();
  // Stop the privhelper process.
  shutdownPrivhelper();
  shutdownResult.throwUnlessValue();

  return true;
}

folly::SemiFuture<Unit> EdenServer::performNormalShutdown() {
#ifndef _WIN32
  takeoverServer_.reset();
#endif // !_WIN32

  // Clean up all the server mount points
  return unmountAll();
}

void EdenServer::shutdownPrivhelper() {
  // Explicitly stop the privhelper process so we can verify that it
  // exits normally.
  const auto privhelperExitCode = serverState_->getPrivHelper()->stop();
  if (privhelperExitCode != 0) {
    if (privhelperExitCode > 0) {
      XLOG(
          ERR,
          "privhelper process exited with unexpected code {}",
          privhelperExitCode);
    } else {
      XLOGF(
          ERR,
          "privhelper process was killed by signal {}",
          privhelperExitCode);
    }
  }
}

void EdenServer::addToMountPoints(std::shared_ptr<EdenMount> edenMount) {
  auto& mountPath = edenMount->getPath();

  {
    const auto mountPoints = mountPoints_->wlock();
    const auto ret = mountPoints->emplace(mountPath, EdenMountInfo(edenMount));
    if (!ret.second) {
      throw newEdenError(
          EEXIST,
          EdenErrorType::POSIX_ERROR,
          fmt::format("mount point \"{}\" is already mounted", mountPath));
    }
  }
}

void EdenServer::registerStats(std::shared_ptr<EdenMount> edenMount) {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->registerCallback(
      edenMount->getCounterName(CounterName::INODEMAP_LOADED), [edenMount] {
        auto counts = edenMount->getInodeMap()->getInodeCounts();
        return counts.fileCount + counts.treeCount;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::INODEMAP_UNLOADED), [edenMount] {
        return edenMount->getInodeMap()->getInodeCounts().unloadedInodeCount;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::PERIODIC_INODE_UNLOAD),
      [edenMount] {
        return edenMount->getInodeMap()
            ->getInodeCounts()
            .periodicLinkedUnloadInodeCount;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::PERIODIC_UNLINKED_INODE_UNLOAD),
      [edenMount] {
        return edenMount->getInodeMap()
            ->getInodeCounts()
            .periodicUnlinkedUnloadInodeCount;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MEMORY),
      [edenMount] { return edenMount->getJournal().estimateMemoryUsage(); });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::JOURNAL_ENTRIES), [edenMount] {
        auto stats = edenMount->getJournal().getStats();
        return stats ? stats->entryCount : 0;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::JOURNAL_DURATION), [edenMount] {
        auto stats = edenMount->getJournal().getStats();
        return stats ? stats->getDurationInSeconds() : 0;
      });
  counters->registerCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MAX_FILES_ACCUMULATED),
      [edenMount] {
        auto stats = edenMount->getJournal().getStats();
        return stats ? stats->maxFilesAccumulated : 0;
      });
#ifndef _WIN32
  if (auto* channel = edenMount->getFuseChannel()) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      counters->registerCallback(
          getCounterNameForFuseRequests(
              RequestMetricsScope::RequestStage::LIVE, metric, edenMount.get()),
          [edenMount, metric, channel] {
            return channel->getRequestMetric(metric);
          });
    }
  } else if (edenMount->getNfsdChannel()) {
    // TODO(xavierd): Add requestMetrics for NFS.
  }
#endif
#ifdef __linux__
  if (edenMount->getFuseChannel()) {
    counters->registerCallback(
        getCounterNameForFuseRequests(
            RequestMetricsScope::RequestStage::PENDING,
            RequestMetricsScope::RequestMetric::COUNT,
            edenMount.get()),
        [edenMount] {
          try {
            return getNumberPendingFuseRequests(edenMount.get());
          } catch (const std::exception&) {
            return size_t{0};
          }
        });
  }
#endif // __linux__
}

void EdenServer::unregisterStats(EdenMount* edenMount) {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::INODEMAP_LOADED));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::INODEMAP_UNLOADED));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::PERIODIC_INODE_UNLOAD));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::PERIODIC_UNLINKED_INODE_UNLOAD));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MEMORY));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_ENTRIES));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_DURATION));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MAX_FILES_ACCUMULATED));
#ifndef _WIN32
  if (edenMount->getFuseChannel()) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      counters->unregisterCallback(getCounterNameForFuseRequests(
          RequestMetricsScope::RequestStage::LIVE, metric, edenMount));
    }
  } else if (edenMount->getNfsdChannel()) {
    // TODO(xavierd): Unregister NFS metrics
  }
#endif
#ifdef __linux__
  if (edenMount->getFuseChannel()) {
    counters->unregisterCallback(getCounterNameForFuseRequests(
        RequestMetricsScope::RequestStage::PENDING,
        RequestMetricsScope::RequestMetric::COUNT,
        edenMount));
  }
#endif // __linux__
}

void EdenServer::registerInodePopulationReportsCallback() {
  getServerState()->getNotifier()->registerInodePopulationReportCallback(
      [mountPoints = mountPoints_]() -> std::vector<InodePopulationReport> {
        std::vector<InodePopulationReport> inodePopulationReports;

        {
          const auto lockedMountPoints = mountPoints->rlock();
          for (const auto& [_, info] : *lockedMountPoints) {
            const auto& mount = info.edenMount;

            auto counts = mount->getInodeMap()->getInodeCounts();

            inodePopulationReports.push_back(
                {mount->getCheckoutConfig()->getMountPath().c_str(),
                 counts.fileCount + counts.treeCount +
                     counts.unloadedInodeCount});
          }
        }

        return inodePopulationReports;
      });
}

void EdenServer::unregisterInodePopulationReportsCallback() {
  getServerState()->getNotifier()->registerInodePopulationReportCallback(
      nullptr);
}

Future<Unit> EdenServer::performTakeoverStart(
    [[maybe_unused]] std::shared_ptr<EdenMount> edenMount,
    [[maybe_unused]] TakeoverData::MountInfo&& info) {
#ifndef _WIN32
  auto mountPath = info.mountPath;

  auto future = completeTakeoverStart(edenMount, std::move(info));
  return std::move(future).thenValue(
      [this,
       edenMount = std::move(edenMount),
       mountPath = std::move(mountPath)](auto&&) mutable {
        // Daemon-managed bind mounts are vestigial, but left in the privhelper
        // protocol in case we change our mind.
        std::vector<std::string> bindMounts;
        return serverState_->getPrivHelper()->takeoverStartup(
            mountPath.view(), bindMounts);
      });
#else
  NOT_IMPLEMENTED();
#endif
}

Future<Unit> EdenServer::completeTakeoverStart(
    [[maybe_unused]] std::shared_ptr<EdenMount> edenMount,
    [[maybe_unused]] TakeoverData::MountInfo&& info) {
  if (auto channelData = std::get_if<FuseChannelData>(&info.channelInfo)) {
    // Start up the fuse workers.
    return folly::makeFutureWith(
        [&] { edenMount->takeoverFuse(std::move(*channelData)); });
  } else if (
      auto nfsMountInfo = std::get_if<NfsChannelData>(&info.channelInfo)) {
    return edenMount->takeoverNfs(std::move(*nfsMountInfo));
  } else {
    return folly::makeFuture<Unit>(std::runtime_error(fmt::format(
        "Unsupported ChannelInfo Type: {}", info.channelInfo.index())));
  }
}

ImmediateFuture<std::shared_ptr<EdenMount>> EdenServer::mount(
    std::unique_ptr<CheckoutConfig> initialConfig,
    bool readOnly,
    OverlayChecker::ProgressCallback&& progressCallback,
    optional<TakeoverData::MountInfo>&& optionalTakeover) {
  folly::stop_watch<> mountStopWatch;

  initialConfig->clearIntentionallyUnmountedFlag();

  auto bsType = toBackingStoreType(initialConfig->getRepoType());
  XLOGF(
      INFO, "Creating backing store of type: {}", toBackingStoreString(bsType));
  auto backingStore =
      getBackingStore(bsType, initialConfig->getRepoSource(), *initialConfig);

  auto objectStore = ObjectStore::create(
      backingStore,
      localStore_,
      treeCache_,
      getStats().copy(),
      serverState_->getProcessInfoCache(),
      serverState_->getStructuredLogger(),
      serverState_->getReloadableConfig(),
      initialConfig->getEnableWindowsSymlinks(),
      initialConfig->getCaseSensitive());
  auto journal = std::make_unique<Journal>(getStats().copy());

  // Create the EdenMount object and insert the mount into the mountPoints_ map.
  auto edenMount = EdenMount::create(
      std::move(initialConfig),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal),
      getStats().copy());
  addToMountPoints(edenMount);

  registerStats(edenMount);

  const bool doTakeover = optionalTakeover.has_value();

  auto initFuture = edenMount->initialize(
      std::move(progressCallback),
      doTakeover ? std::make_optional(optionalTakeover->inodeMap)
                 : std::nullopt,
      doTakeover ? std::make_optional(optionalTakeover->getMountProtocol())
                 : std::nullopt);

  // Now actually begin starting the mount point
  return std::move(initFuture)
      .thenError([this, edenMount](folly::exception_wrapper ew) {
        XLOGF(
            ERR, "error initializing {}: {}", edenMount->getPath(), ew.what());
        mountFinished(edenMount.get(), std::nullopt);
        return makeImmediateFuture<folly::Unit>(std::move(ew));
      })
      .thenValue([this,
                  doTakeover,
                  readOnly,
                  edenMount,
                  mountStopWatch,
                  optionalTakeover =
                      std::move(optionalTakeover)](folly::Unit) mutable {
        return (optionalTakeover ? performTakeoverStart(
                                       edenMount, std::move(*optionalTakeover))
                                 : edenMount->startFsChannel(readOnly))
            .thenError([this, edenMount](folly::exception_wrapper ew) {
              // Call mountFinished() if an error occurred during FUSE
              // initialization.
              mountFinished(edenMount.get(), std::nullopt);
              return makeFuture<folly::Unit>(std::move(ew));
            })
            .thenValue([edenMount, doTakeover, this](folly::Unit) mutable {
              registerStats(edenMount);

              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              auto finishFuture =
                  edenMount->getFsChannelCompletionFuture().thenTry(
                      [this, edenMount](
                          folly::Try<TakeoverData::MountInfo>&& takeover) {
                        std::optional<TakeoverData::MountInfo> optTakeover;
                        if (takeover.hasValue()) {
                          optTakeover = std::move(takeover.value());
                        }
                        unregisterStats(edenMount.get());
                        mountFinished(edenMount.get(), std::move(optTakeover));
                      });

              if (doTakeover) {
                // The bind mounts are already mounted in the takeover case
                return makeFuture<std::shared_ptr<EdenMount>>(
                    std::move(edenMount));
              } else {
                // Perform all of the bind mounts associated with the
                // client.  We don't need to do this for the takeover
                // case as they are already mounted.
                return edenMount->performBindMounts()
                    .deferValue([edenMount](auto&&) { return edenMount; })
                    .deferError([edenMount](folly::exception_wrapper ew) {
                      XLOG(
                          ERR,
                          "Error while performing bind mounts, will continue with mount anyway: {}",
                          ew.what());
                      return edenMount;
                    })
                    .via(getServerState()->getThreadPool().get());
              }
            })
            .thenTry([this, mountStopWatch, doTakeover, edenMount](auto&& t) {
              auto* fsChannel = edenMount->getFsChannel();
              auto inodeCatalogType =
                  edenMount->getCheckoutConfig()->getInodeCatalogType();
              serverState_->getStructuredLogger()->logEvent(FinishedMount{
                  std::string{
                      toBackingStoreString(edenMount->getCheckoutConfig()
                                               ->getRepoBackingStoreType())},
                  edenMount->getCheckoutConfig()->getRepoType(),
                  std::string{basename(
                      edenMount->getCheckoutConfig()->getRepoSource())},
                  fsChannel ? fsChannel->getName() : "unknown",
                  doTakeover,
                  std::chrono::duration<double>{mountStopWatch.elapsed()}
                      .count(),
                  !t.hasException(),
                  edenMount->getOverlay()->hadCleanStartup(),
                  inodeCatalogType.has_value()
                      ? static_cast<int64_t>(inodeCatalogType.value())
                      : 0});
              return makeFuture(std::move(t));
            });
      });
}

folly::SemiFuture<Unit> EdenServer::unmount(
    AbsolutePathPiece mountPath,
    UnmountOptions options) {
  return folly::makeSemiFutureWith([&] {
           auto future = Future<Unit>::makeEmpty();
           auto mount = std::shared_ptr<EdenMount>{};
           {
             const auto mountPoints = mountPoints_->wlock();
             const auto it = mountPoints->find(mountPath);
             if (it == mountPoints->end()) {
               return folly::makeSemiFuture<Unit>(std::out_of_range(
                   fmt::format("no such mount point {}", mountPath)));
             }
             future = it->second.unmountPromise.getFuture();
             mount = it->second.edenMount;
           }

           // We capture the mount shared_ptr in the lambda to keep the
           // EdenMount object alive during the call to unmount.
           return mount->unmount(options).deferValue(
               [mount, f = std::move(future)](auto&&) mutable {
                 return std::move(f);
               });
         })
      .deferError([path = mountPath.copy()](folly::exception_wrapper&& ew) {
        XLOGF(ERR, "Failed to perform unmount for \"{}\": {}", path, ew.what());
        return folly::makeSemiFuture<Unit>(std::move(ew));
      });
}

void EdenServer::mountFinished(
    EdenMount* edenMount,
    std::optional<TakeoverData::MountInfo> takeover) {
  const auto& mountPath = edenMount->getPath();
  XLOGF(INFO, "mount point \"{}\" stopped", mountPath);

  // Save the unmount and takeover Promises
  folly::SharedPromise<Unit> unmountPromise;
  std::optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;
  auto shutdownFuture = folly::SemiFuture<SerializedInodeMap>::makeEmpty();
  {
    const auto mountPoints = mountPoints_->wlock();
    const auto it = mountPoints->find(mountPath);
    XCHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    takeoverPromise = std::move(it->second.takeoverPromise);

    const bool doTakeover = takeoverPromise.has_value();
    // Shutdown the EdenMount, while holding the mountPoints_ lock. This is
    // done to avoid a race between checking isSafeForInodeAccess and the
    // InodeMap being destroyed. In that race, the rootInode may be a nullptr
    // thus leading to a nullptr dereference.
    shutdownFuture = edenMount->shutdown(doTakeover);
  }

  std::move(shutdownFuture)
      .via(getMainEventBase())
      .thenTry([unmountPromise = std::move(unmountPromise),
                takeoverPromise = std::move(takeoverPromise),
                takeoverData = std::move(takeover)](
                   folly::Try<SerializedInodeMap>&& result) mutable {
        if (takeoverPromise) {
          takeoverPromise.value().setWith([&]() mutable {
            takeoverData.value().inodeMap = std::move(result.value());
            return std::move(takeoverData.value());
          });
        }
        unmountPromise.setTry(
            folly::makeTryWith([result = std::move(result)]() {
              result.throwUnlessValue();
              return Unit{};
            }));
      })
      .ensure([this, mountPath] {
        // Erase the EdenMount from our mountPoints_ map
        const auto mountPoints = mountPoints_->wlock();
        const auto it = mountPoints->find(mountPath);
        if (it != mountPoints->end()) {
          mountPoints->erase(it);
        }
      });
}

std::vector<EdenMountHandle> EdenServer::getMountPoints() const {
  std::vector<EdenMountHandle> results;
  {
    const auto mountPoints = mountPoints_->rlock();
    for (const auto& [path, mountInfo] : *mountPoints) {
      auto& mount = mountInfo.edenMount;
      // Avoid returning mount points that are still initializing and are
      // not ready to perform inode operations yet.
      if (!mount->isSafeForInodeAccess()) {
        continue;
      }
      results.emplace_back(mount, mount->getRootInodeUnchecked());
    }
  }
  return results;
}

EdenServer::MountList EdenServer::getAllMountPoints() const {
  MountList results;
  {
    const auto mountPoints = mountPoints_->rlock();
    for (const auto& entry : *mountPoints) {
      results.emplace_back(entry.second.edenMount);
    }
  }
  return results;
}

EdenMountHandle EdenServer::getMount(AbsolutePathPiece mountPath) const {
  const auto mountPoints = mountPoints_->rlock();
  const auto it = mountPoints->find(mountPath);
  if (it == mountPoints->end()) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        fmt::format(
            "mount point \"{}\" is not known to this eden instance",
            mountPath));
  }
  auto mount = it->second.edenMount;
  if (!mount->isSafeForInodeAccess()) {
    throw newEdenError(
        EBUSY,
        EdenErrorType::POSIX_ERROR,
        fmt::format("mount point \"{}\" is still initializing", mountPath));
  }
  return {mount, mount->getRootInodeUnchecked()};
}

ImmediateFuture<CheckoutResult> EdenServer::checkOutRevision(
    AbsolutePathPiece mountPath,
    std::string& rootId,
    std::optional<folly::StringPiece> rootHgManifest,
    const ObjectFetchContextPtr& fetchContext,
    StringPiece callerName,
    CheckoutMode checkoutMode) {
  auto mountHandle = getMount(mountPath);
  auto& edenMount = mountHandle.getEdenMount();
  auto root = edenMount.getObjectStore()->parseRootId(rootId);
  if (rootHgManifest.has_value()) {
    // The hg client has told us what the root manifest is.
    //
    // This is useful when a commit has just been created.  We won't be able to
    // ask the import helper to map the commit to its root manifest because it
    // won't know about the new commit until it reopens the repo.  Instead,
    // import the manifest for this commit directly.
    auto rootManifest = hash20FromThrift(rootHgManifest.value());
    edenMount.getObjectStore()
        ->getBackingStore()
        ->importManifestForRoot(root, rootManifest, fetchContext)
        .get();
  }

  bool isNfs = edenMount.isNfsdChannel();

  // the +1 is so we count the current checkout that hasn't quite started yet
  getServerState()->getNotifier()->signalCheckout(
      enumerateInProgressCheckouts() + 1);

  auto checkoutFuture =
      edenMount
          .checkout(
              mountHandle.getRootInode(),
              root,
              fetchContext,
              callerName,
              checkoutMode)
          .thenValue([this, checkoutMode, isNfs, mountPath = mountPath.copy()](
                         CheckoutResult&& result) {
            getServerState()->getNotifier()->signalCheckout(
                enumerateInProgressCheckouts());
            if (checkoutMode == CheckoutMode::DRY_RUN) {
              return std::move(result);
            }

            // In NFSv3 the kernel never tells us when its safe to unload
            // inodes ("safe" meaning all file handles to the inode have been
            // closed).
            //
            // To avoid unbounded memory and disk use we need to periodically
            // clean them up. Checkout will likely create a lot of stale innodes
            // so we run a delayed cleanup after checkout.
            if (isNfs &&
                serverState_->getReloadableConfig()
                    ->getEdenConfig()
                    ->unloadUnlinkedInodes.getValue()) {
              // During whole Eden Process shutdown, this function can only be
              // run before the mount is destroyed. This is because the function
              // is either run before the server event base is destroyed or it
              // is not run at all, and the server event base is destroyed
              // before the mountPoints. Since the function must be run before
              // the eventbase is destroyed and the eventbase is destroyed
              // before the mountPoints, this function can only be called before
              // the mount points are destroyed during normal destruction.
              // However, the mount point might have been unmounted before this
              // function is run outside of shutdown.
              auto delay = serverState_->getReloadableConfig()
                               ->getEdenConfig()
                               ->postCheckoutDelayToUnloadInodes.getValue();
              XLOGF(
                  DBG9,
                  "Scheduling unlinked inode cleanup for mount {} in {} seconds.",
                  mountPath,
                  durationStr(delay));
              this->scheduleCallbackOnMainEventBase(
                  std::chrono::duration_cast<std::chrono::milliseconds>(delay),
                  [this, mountPath = mountPath.copy()]() {
                    try {
                      // TODO: This might be a pretty expensive operation to run
                      // on an EventBase. Maybe we should debounce onto a
                      // different executor.
                      auto mountHandle = this->getMount(mountPath);
                      mountHandle.getEdenMount().forgetStaleInodes();
                    } catch (EdenError& err) {
                      // This is an expected error if the mount has been
                      // unmounted before this callback ran.
                      if (err.errorCode() == ENOENT) {
                        XLOGF(
                            DBG3,
                            "Callback to clear inodes: Mount cannot be found. {}",
                            *err.message());
                      } else {
                        throw;
                      }
                    }
                  });
            }
            // Verify if cache file invalidation works as expected after
            // checkout
            // TODO T213849128: This is to collect data for S439820.
            // We can remove this once SEV close.
            if (isNfs &&
                serverState_->getReloadableConfig()
                    ->getEdenConfig()
                    ->verifyFilesAfterCheckout.getValue()) {
              std::vector<InodeNumber> inodesToValidate;
              inodesToValidate.swap(result.sampleInodesToValidate);
              folly::via(
                  this->getServerState()->getValidationThreadPool().get(),
                  [this,
                   inodesToValidate = std::move(inodesToValidate),
                   mountPath = mountPath.copy(),
                   maxSizeOfFileToVerifyInvalidation =
                       this->serverState_->getReloadableConfig()
                           ->getEdenConfig()
                           ->maxSizeOfFileToVerifyInvalidation.getValue(),
                   structuredLogger =
                       this->serverState_->getStructuredLogger()]() {
                    // for each inode
                    std::vector<ImmediateFuture<std::tuple<
                        RelativePath,
                        InodeNumber,
                        std::optional<Hash20>>>>
                        expectedContents;
                    {
                      // note we don't want to hold the mount handle in the
                      // capture because that would block shutdown until this
                      // future completes. This check can block waiting for the
                      // eden fuse threads to complete an fs request. We don't
                      // want to make a cycle, so to be safe we just attempt to
                      // lookup the mount and gracefully handle errors.
                      auto mountHandle = this->getMount(mountPath);
                      auto inodeMap = mountHandle.getEdenMount().getInodeMap();
                      for (auto& inodeNum : inodesToValidate) {
                        auto inode = inodeMap->lookupLoadedInode(inodeNum);
                        if (!inode) {
                          // Skip if inode doesn't exist because it is already
                          // invalidated
                          continue;
                        }
                        auto path = inode->getPath();
                        if (!path.has_value()) {
                          // skip if we can't get the path because we
                          // can't validate it anyway
                          continue;
                        }
                        if (auto fileInode = inode.asFile()) {
                          expectedContents.emplace_back(
                              fileInode
                                  ->getSha1(
                                      ObjectFetchContext::getNullContext())
                                  .thenValue(
                                      [path = inode->getPath(),
                                       inodeNum](auto&& sha1) mutable
                                      -> std::tuple<
                                          RelativePath,
                                          InodeNumber,
                                          std::optional<Hash20>> {
                                        return {
                                            std::move(path).value(),
                                            inodeNum,
                                            std::move(sha1)};
                                      }));
                        } else {
                          expectedContents.emplace_back(std::tuple<
                                                        RelativePath,
                                                        InodeNumber,
                                                        std::optional<Hash20>>(
                              std::move(path).value(), inodeNum, std::nullopt));
                        }
                      }
                    }

                    return collectAll(std::move(expectedContents))
                        .thenValue([mountPath = mountPath.copy(),
                                    maxSizeOfFileToVerifyInvalidation,
                                    structuredLogger](auto&& expectedResults) {
                          for (auto& result : expectedResults) {
                            if (!result.hasValue()) {
                              continue;
                            }
                            auto [path, inodeNum, sha1Result] = result.value();
                            auto fileStat =
                                getFileStat((mountPath + path).value().c_str());
                            if (!fileStat.hasValue()) {
                              // Skip if file stat is not available
                              continue;
                            } else if (
                                fileStat.value().size >
                                maxSizeOfFileToVerifyInvalidation) {
                              // Skip verification for large files
                              continue;
                            }
                            // sha1 the path
                            std::string contents;
                            folly::readFile(
                                (mountPath + path).value().c_str(), contents);

                            auto actualSha1 = Hash20::sha1(contents);

                            if (actualSha1 != sha1Result) {
                              // log to scuba.
                              structuredLogger->logEvent(StaleContents{
                                  path.value(), inodeNum.getRawValue()});
                              std::string sha1ResultStr =
                                  sha1Result ? sha1Result->toString() : "none";
                              XLOG(
                                  WARN,
                                  "Stale file contents after checkout for path {} (ino {}) - actual sha1: {} , expected sha1: {}",
                                  path.value(),
                                  inodeNum.getRawValue(),
                                  actualSha1.toString(),
                                  sha1ResultStr);
                            }
                          }
                        });
                  });
            }

            return std::move(result);
          });

  if (thriftUseCheckoutExecutor_) {
    // This is an UnboundedQueueExecutor, which is guaranteed to never block
    // nor throw (except OOM), nor execute inline from `add()`.
    //
    // Thrift documentation states that "it is almost never a good idea to send
    // work off Thrift’s CPU worker". However, it also notes that "user code may
    // delegate the rest of the work to other thread pools, thus shifting load
    // off Thrift’s CPU Workers thread and letting the thread pick up new
    // incoming work" but warns that "if there is work offloaded to other thread
    // pools, there should be backpressure mechanism that would block Thrift CPU
    // Workers thread when those internal thread pools are overloaded."
    //
    // Without backpressure, EdenFS would see an increase in memory pressure.
    // potential slowdowns, and/or OOMS. EdenFS only allows one checkout at a
    // time (https://fburl.com/code/a6eoy8wu), which acts as the aforementioned
    // backpressure mechanism.
    checkoutFuture =
        std::move(checkoutFuture).semi().via(checkoutRevisionExecutor_.get());
  }

  return std::move(checkoutFuture).ensure([mountHandle] {});
}

shared_ptr<BackingStore> EdenServer::getBackingStore(
    BackingStoreType type,
    StringPiece name,
    const CheckoutConfig& config) {
  BackingStoreKey key{type, name.str()};
  auto lockedStores = backingStores_.wlock();
  const auto it = lockedStores->find(key);
  if (it != lockedStores->end()) {
    return it->second;
  }

  const auto store = backingStoreFactory_->createBackingStore(
      type,
      BackingStoreFactory::CreateParams{
          name, serverState_.get(), localStore_, getStats(), config});
  lockedStores->emplace(key, store);
  return store;
}

std::unordered_set<std::shared_ptr<BackingStore>>
EdenServer::getBackingStores() {
  std::unordered_set<std::shared_ptr<BackingStore>> backingStores{};
  auto lockedStores = this->backingStores_.rlock();
  for (auto entry : *lockedStores) {
    backingStores.emplace(std::move(entry.second));
  }
  return backingStores;
}

std::unordered_set<shared_ptr<SaplingBackingStore>>
EdenServer::getSaplingBackingStores() {
  std::unordered_set<std::shared_ptr<SaplingBackingStore>>
      saplingBackingStores{};
  {
    auto lockedStores = this->backingStores_.rlock();
    for (const auto& entry : *lockedStores) {
      if (auto saplingBackingStore =
              std::dynamic_pointer_cast<SaplingBackingStore>(entry.second)) {
        saplingBackingStores.emplace(std::move(saplingBackingStore));
      }
    }
  }
  return saplingBackingStores;
}

std::vector<size_t> EdenServer::collectSaplingBackingStoreCounters(
    std::function<size_t(const SaplingBackingStore&)> getCounterFromStore) {
  std::vector<size_t> counters;
  for (const auto& store : this->getSaplingBackingStores()) {
    counters.emplace_back(getCounterFromStore(*store));
  }
  return counters;
}

folly::SemiFuture<Unit> EdenServer::createThriftServer() {
  auto edenConfig = config_->getEdenConfig();
  server_ = make_shared<ThriftServer>();
#ifdef EDEN_HAVE_SERVER_OBSERVER
  server_->setObserver(createServerObserver(
      kServiceName, edenConfig->thriftServerObserverSamplingRate.getValue()));
  // The server observer that is set up above collects its stats via a
  // ThreadCachedServiceData object rather than directly via the ServiceData
  // object for efficiency's sake. However, it does require periodically calling
  // publishStats() in order to flush the stats data cached in each thread to
  // the underlying ServiceData object.
  fb303::ThreadCachedServiceData::get()->startPublishThread(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          edenConfig->thriftServerObserverPublishInterval.getValue()));
#endif
  server_->setMaxRequests(edenConfig->thriftMaxRequests.getValue());

  // Set up the CPU worker threads
  size_t thriftCpuThreads = edenConfig->thriftNumWorkers.getValue();
  if (!thriftUseResourcePools_) {
    server_->setNumCPUWorkerThreads(thriftCpuThreads);
  } else {
    server_->requireResourcePools();
    auto executor = std::make_shared<folly::CPUThreadPoolExecutor>(
        std::make_pair(thriftCpuThreads, thriftCpuThreads),
        folly::CPUThreadPoolExecutor::makeThrottledLifoSemQueue(
            std::chrono::microseconds{0}),
        std::make_shared<folly::NamedThreadFactory>("EdenThrift"));
    auto requestPile = std::make_unique<apache::thrift::RoundRobinRequestPile>(
        apache::thrift::RoundRobinRequestPile::Options{});
    std::unique_ptr<apache::thrift::ConcurrencyControllerInterface>
        concurrencyController;
    auto mode = apache::thrift::ParallelConcurrencyController::
        RequestExecutionMode::Parallel;
    if (thriftUseSerialExecution_) {
      XLOG(INFO, "Using Serial Execution for Thrift Server");
      mode = apache::thrift::ParallelConcurrencyController::
          RequestExecutionMode::Serial;
    } else {
      XLOG(INFO, "Using Resource Pools for Thrift Server");
    }

    concurrencyController =
        std::make_unique<apache::thrift::ParallelConcurrencyController>(
            *requestPile, *executor, mode);

    server_->resourcePoolSet().setResourcePool(
        apache::thrift::ResourcePoolHandle::defaultAsync(),
        std::move(requestPile),
        std::move(executor),
        std::move(concurrencyController),
        apache::thrift::concurrency::NORMAL);
  }
  server_->setCPUWorkerThreadName("EdenThrift");

  server_->setQueueTimeout(std::chrono::floor<std::chrono::milliseconds>(
      edenConfig->thriftQueueTimeout.getValue()));
  server_->setAllowCheckUnimplementedExtraInterfaces(false);

  // Setting this allows us to to only do stopListening() on the stop() call
  // and delay thread-pool join (stop cpu workers + stop workers) until
  // server object destruction. This specifically matters in the takeover
  // shutdown code path.
  server_->setStopWorkersOnStopListening(false);
  server_->leakOutstandingRequestsWhenServerStops(
      edenConfig->thriftLeakOutstandingRequestsWhenServerStops.getValue());

  // S532385: The above leakOutstandingRequestsWhenServerStops config has
  // memory safety issues. We'd like to remove it, but removing it leads to a
  // new class of bugs where outstanding requests do not complete and the
  // server crashes as a result. This config attempts to mitigate that by
  // allowing more time for each request to complete on shutdown.
  server_->setWorkersJoinTimeout(
      std::chrono::duration_cast<std::chrono::seconds>(
          edenConfig->thriftWorkersJoinTimeout.getValue()));

#ifdef EDEN_HAVE_USAGE_SERVICE
  auto usageService = std::make_unique<EdenFSSmartPlatformServiceEndpoint>(
      serverState_->getThreadPool(), serverState_->getEdenConfig());
#else
  auto usageService = std::make_unique<NullUsageService>();
#endif

  handler_ = make_shared<EdenServiceHandler>(
      originalCommandLine_, this, std::move(usageService));
  server_->setInterface(handler_);

  // Get the path to the thrift socket.
  auto thriftSocketPath = edenDir_.getThriftSocketPath();
  folly::SocketAddress thriftAddress;
  thriftAddress.setFromPath(thriftSocketPath.view());
  server_->setAddress(thriftAddress);
  serverState_->setSocketPath(thriftSocketPath);

  serverEventHandler_ = make_shared<ThriftServerEventHandler>(this);
  server_->setServerEventHandler(serverEventHandler_);
  return serverEventHandler_->getThriftRunningFuture();
}

void EdenServer::prepareThriftAddress() const {
  // If we are serving on a local Unix socket, remove any old socket file
  // that may be left over from a previous instance.
  // We have already acquired the mount point lock at this time, so we know
  // that any existing socket is unused and safe to remove.
  const auto& path = getUnixDomainSocketPath(server_->getAddress());
  if (!path) {
    return;
  }

  auto sock = folly::AsyncServerSocket::UniquePtr(new folly::AsyncServerSocket);
  const auto addr = folly::SocketAddress::makeFromPath(*path);

  const size_t maxTries = folly::kIsWindows ? 3 : 1;

  auto tries = 1;
  while (true) {
    const int rc = unlink(path->c_str());
    if (rc != 0 && errno != ENOENT) {
      // This might happen if we don't have permission to remove the file.
      folly::throwSystemError(
          "unable to remove old Eden thrift socket ", *path);
    }

    try {
      sock->bind(addr);
      break;
    } catch (const std::system_error&) {
      if (tries == maxTries) {
        throw;
      }

      // On Windows, a race condition exist where an attempt to connect to a
      // non-existing socket will create it on-disk, preventing bind(2) from
      // succeeding, leading to the Thrift server failing to start.
      //
      // Since at this point we're holding the lock, no other edenfs process
      // should be attempting to bind to the socket, and thus it's safe to try
      // to remove it, and retry.

      tries++;
    }
  }

  server_->useExistingSocket(std::move(sock));
}

void EdenServer::stop() {
  {
    auto state = runningState_.wlock();
    if (state->state == RunState::SHUTTING_DOWN) {
      // If we are already shutting down, we don't want to continue down this
      // code path in case we are doing a graceful restart. That could result in
      // a race which could trigger a failure in performCleanup.
      XLOG(INFO, "stop was called while server was already shutting down");
      return;
    }
    state->state = RunState::SHUTTING_DOWN;
  }
  clearStartupStatusPublishers();
  shutdownSubscribers();
  server_->stop();
}

bool EdenServer::shouldChunkTakeoverData() {
  return serverState_->getEdenConfig()->shouldChunkTakeoverData.getValue();
}

folly::Future<TakeoverData> EdenServer::startTakeoverShutdown() {
#ifndef _WIN32
  // Make sure we aren't already shutting down, then update our state
  // to indicate that we should perform mount point takeover shutdown
  // once runServer() returns.
  folly::Promise<std::optional<TakeoverData>> takeoverPromise;
  folly::File thriftSocket;
  {
    auto state = runningState_.wlock();
    if (state->state != RunState::RUNNING) {
      // We are either still in the process of starting,
      // or already shutting down.
      return makeFuture<TakeoverData>(std::runtime_error(folly::to<string>(
          "can only perform graceful restart when running normally; "
          "current state is ",
          enumValue(state->state))));
    }

    {
      const auto mountPoints = mountPoints_->rlock();
      for (const auto& entry : *mountPoints) {
        const auto& mount = entry.second.edenMount;
        if (!mount->isSafeForInodeAccess()) {
          return makeFuture<TakeoverData>(std::runtime_error(
              "can only perform graceful restart when all mount points are initialized"));
          // TODO(xavierd): There is still a potential race after this check if
          // a mount is initiated at this point. Injecting a block below and
          // starting a mount would manifest it. In practice, this should be
          // fairly rare. Moving this further (in stopMountsForTakeover for
          // instance) to avoid this race requires EdenFS to being able to
          // gracefully handle failures and recover in these cases by restarting
          // several components after they have been already shutdown.
        }
      }
    }

    // We should confirm that no garbage collection is running during graceful
    // restart.
    // First stop the periodic GC
    gcTask_.updateInterval(std::chrono::seconds(0));
    // Also, wait for any running GC to finish.
    bool isGCRunning = isWorkingCopyGCRunningForAnyMount();
    uint8_t checkingGCAttempts = 0;
    while (isGCRunning && checkingGCAttempts < 3) {
      std::this_thread::sleep_for(std::chrono::seconds(20));
      isGCRunning = isWorkingCopyGCRunningForAnyMount();
      checkingGCAttempts++;
    }
    if (isGCRunning) {
      // We are still waiting for the GC to finish. This is unexpected and
      // should not happen. We should not proceed with the graceful restart.
      return makeFuture<TakeoverData>(std::runtime_error(
          "cannot run graceful restart while garbage collection is running"));
    }
    // Note: If anything fails after this point, and we fallback to the old
    // eden, we don't need to worry about updating the gcTask_ interval.
    // Updating gcTask_ interval is part of the reloadConfig() periodic task
    // which is running every few minutes.

    // Make a copy of the thrift server socket so we can transfer it to the
    // new edenfs process.  Our local thrift will close its own socket when
    // we stop the server.  The easiest way to avoid completely closing the
    // server socket for now is simply by duplicating the socket to a new
    // fd. We will transfer this duplicated FD to the new edenfs process.
    const int takeoverThriftSocket = dup(server_->getListenSocket());
    folly::checkUnixError(
        takeoverThriftSocket,
        "error duplicating thrift server socket during graceful takeover");
    thriftSocket = folly::File{takeoverThriftSocket, /* ownsFd */ true};

    // The TakeoverServer will fulfill shutdownFuture with std::nullopt
    // upon successfully sending the TakeoverData, or with the TakeoverData
    // if the takeover was unsuccessful.
    XCHECK(!state->shutdownFuture.valid());
    state->shutdownFuture = takeoverPromise.getFuture();
    state->state = RunState::SHUTTING_DOWN;
  }

  return serverState_->getFaultInjector()
      .checkAsync("takeover", "server_shutdown")
      .semi()
      .via(getMainEventBase())
      .thenValue([this](auto&&) {
        // Compact storage for all key spaces in order to speed up the
        // takeover start of the new process. We could potentially test this
        // more and change it in the future to simply flush instead of
        // compact if this proves to be too expensive.

        // Catch errors from compaction because we do not want this failure
        // to be blocking graceful restart. This can fail if there is no
        // space to write to RocksDB log files.
        try {
          localStore_->compactStorage();
        } catch (const std::exception& e) {
          XLOGF(
              ERR,
              "Failed to compact local store with error: {}. Continuing takeover server shutdown anyway.",
              e.what());
        }

        shutdownSubscribers();

        // Stop the thrift server. In the future, we'd like to
        // keep listening and instead start internally queueing thrift calls
        // to pass with the takeover data below, while waiting here for
        // currently processing thrift calls to finish.
        server_->stop();
      })
      .thenTry([this, takeoverPromise = std::move(takeoverPromise)](
                   auto&& t) mutable {
        if (t.hasException()) {
          takeoverPromise.setException(t.exception());
          t.throwUnlessValue();
        }
        return stopMountsForTakeover(std::move(takeoverPromise));
      })
      .thenValue([this, socket = std::move(thriftSocket)](
                     TakeoverData&& takeover) mutable {
        takeover.lockFile = edenDir_.extractLock();

        takeover.thriftSocket = std::move(socket);
        return via(getMainEventBase())
            .thenValue(
                [this](
                    auto&&) -> folly::SemiFuture<std::optional<folly::File>> {
                  if (auto& takeoverServer =
                          this->getServerState()->getNfsServer()) {
                    return takeoverServer->takeoverStop().deferValue(
                        [](folly::File file) {
                          return std::make_optional<folly::File>(
                              std::move(file));
                        });
                  } else {
                    return std::nullopt;
                  }
                })
            .thenValue([takeover = std::move(takeover)](
                           std::optional<folly::File>&& mountdSocket) mutable {
              if (mountdSocket.has_value()) {
                XLOGF(
                    DBG7,
                    "Got mountd Socket for takeover {}",
                    mountdSocket.value().fd());
              }
              takeover.mountdServerSocket = std::move(mountdSocket);
              return std::move(takeover);
            });
      });
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServer::shutdownSubscribers() {
  // TODO: Set a flag in handler_ to reject future subscription requests.
  // Alternatively, have them seamless transfer through takeovers.

  // If we have any subscription sessions from watchman, we want to shut
  // those down now, otherwise they will block the server_->stop() call
  // below
  XLOG(DBG1, "cancel all subscribers prior to stopping thrift");
  auto mountPoints = mountPoints_->wlock();
  for (auto& [path, info] : *mountPoints) {
    info.edenMount->getJournal().cancelAllSubscribers();
  }
}

const EdenStatsPtr& EdenServer::getStats() const {
  return serverState_->getStats();
}

void EdenServer::flushStatsNow() {
  serverState_->getStats()->flush();
}

void EdenServer::reportMemoryStats() {
  auto memoryStats = facebook::eden::proc_util::readMemoryStats();
  if (memoryStats) {
    // Bump the OBC counters for the current memory usage
    memory_vm_rss_bytes_ += memoryStats->resident;
  }
}

void EdenServer::manageLocalStore() {
  auto config = serverState_->getReloadableConfig()->getEdenConfig(
      ConfigReloadBehavior::NoReload);
  localStore_->periodicManagementTask(*config);
}

void EdenServer::refreshBackingStore() {
  std::vector<shared_ptr<BackingStore>> backingStores;
  {
    auto lockedStores = backingStores_.wlock();
    for (auto& [key, backingStore] : *lockedStores) {
      backingStores.emplace_back(backingStore);
    }
  }

  for (auto& store : backingStores) {
    store->periodicManagementTask();
  }
}

void EdenServer::manageOverlay() {
  const auto mountPoints = mountPoints_->rlock();
  for (const auto& [_, info] : *mountPoints) {
    const auto& mount = info.edenMount;

    mount->getOverlay()->maintenance();
  }
}

ImmediateFuture<uint64_t> EdenServer::garbageCollectWorkingCopy(
    EdenMount& mount,
    TreeInodePtr inode,
    std::chrono::system_clock::time_point cutoff,
    const ObjectFetchContextPtr& context) {
  folly::stop_watch<> workingCopyRuntime;

  auto lease = mount.tryStartWorkingCopyGC(inode);
  if (!lease) {
    XLOGF(
        DBG6,
        "Not running GC for: {}, another GC is already in progress",
        mount.getPath());
    return 0u;
  }

  auto mountPath = mount.getPath();
  auto inodeCountsBeforeGC = mount.getInodeMap()->getInodeCounts();
  auto totalNumberOfInodesBeforeGC = inodeCountsBeforeGC.fileCount +
      inodeCountsBeforeGC.treeCount + inodeCountsBeforeGC.unloadedInodeCount;
  XLOGF(
      DBG1,
      "Starting GC for: {} total number of inodes {}",
      mountPath,
      totalNumberOfInodesBeforeGC);
  return inode->invalidateChildrenNotMaterialized(cutoff, context)
      .ensure([inode, lease = std::move(lease)] {
        inode->unloadChildrenUnreferencedByFs();
      })
      .thenTry([workingCopyRuntime,
                structuredLogger = structuredLogger_,
                mountPath,
                inodeMap = mount.getInodeMap(),
                totalNumberOfInodesBeforeGC](
                   folly::Try<std::pair<uint64_t, bool>> invalidatedTry) {
        auto runtime =
            std::chrono::duration<double>{workingCopyRuntime.elapsed()};

        bool success = invalidatedTry.hasValue();
        int64_t numInvalidated =
            success ? folly::to_signed(invalidatedTry.value().first) : 0;
        auto inodeCountsAfterGC = inodeMap->getInodeCounts();
        auto totalNumberOfInodesAfterGC = inodeCountsAfterGC.fileCount +
            inodeCountsAfterGC.treeCount +
            inodeCountsAfterGC.unloadedInodeCount;

        structuredLogger->logEvent(WorkingCopyGc{
            runtime.count(),
            numInvalidated,
            success,
            static_cast<int64_t>(
                (totalNumberOfInodesBeforeGC - totalNumberOfInodesAfterGC))});
#ifdef __APPLE__
        XLOGF(
            DBG1,
            "GC for: {}, completed in: {} seconds and invalidated {} tree inodes, total number of inodes after GC: {}",
            mountPath,
            runtime.count(),
            numInvalidated,
            totalNumberOfInodesAfterGC);
#else
        XLOGF(
            DBG1,
            "GC for: {}, completed in: {} seconds and invalidated {} inodes, total number of inodes after GC: {}",
            mountPath,
            runtime.count(),
            numInvalidated,
            totalNumberOfInodesAfterGC);
#endif

        return invalidatedTry.value().first;
      });
}

void EdenServer::garbageCollectAllMounts() {
  auto config = serverState_->getReloadableConfig()->getEdenConfig();
  auto cutoffConfig =
      std::chrono::duration_cast<std::chrono::system_clock::duration>(
          config->gcCutoff.getValue());
  auto shorterCutoffConfig =
      std::chrono::duration_cast<std::chrono::system_clock::duration>(
          config->aggressiveGcCutoff.getValue());
  std::chrono::system_clock::time_point cutoff;

  auto mountPoints = getMountPoints();
  for (auto& mountHandle : mountPoints) {
    auto inodeCountsBeforeGc =
        mountHandle.getEdenMount().getInodeMap()->getInodeCounts();
    auto totalNumberOfInodesBeforeGc = inodeCountsBeforeGc.fileCount +
        inodeCountsBeforeGc.treeCount + inodeCountsBeforeGc.unloadedInodeCount;
    auto aggressiveGcThreshold = config->aggressiveGcThreshold.getValue();
    // If aggressiveGcThreshold is set to 0, we will not run GC with shorter
    // cutoff.
    if (aggressiveGcThreshold > 0 &&
        totalNumberOfInodesBeforeGc > aggressiveGcThreshold) {
      // If the number of inodes is above the threshold, we want to run
      // GC with shorter cutoff.
      cutoff = std::chrono::system_clock::now() - shorterCutoffConfig;
    } else {
      cutoff = std::chrono::system_clock::now() - cutoffConfig;
    }
    folly::via(
        getServerState()->getThreadPool().get(),
        [this, mountHandle, cutoff]() mutable {
          static auto context =
              ObjectFetchContext::getNullContextWithCauseDetail(
                  "EdenServer::garbageCollectAllMounts");
          return garbageCollectWorkingCopy(
                     mountHandle.getEdenMount(),
                     mountHandle.getRootInode(),
                     cutoff,
                     context)
              .semi();
        })
        .ensure([mountHandle] {});
  }
}

bool EdenServer::isWorkingCopyGCRunningForAnyMount() const {
  auto mountPoints = getMountPoints();
  for (auto& mountHandle : mountPoints) {
    if (mountHandle.getEdenMount().isWorkingCopyGCRunning()) {
      return true;
    }
  }
  return false;
}

void EdenServer::accidentalUnmountRecovery() {
  XLOGF(DBG5, "Performing accidental unmount recovery.");
  folly::dynamic dirs = folly::dynamic::object();
  try {
    dirs = CheckoutConfig::loadClientDirectoryMap(edenDir_.getPath());
  } catch (...) {
    XLOGF(
        WARN,
        "Could not parse config.json file: {} skipping accidental unmount recovery.",
        folly::exceptionStr(std::current_exception()));
    return;
  }

  if (dirs.empty()) {
    XLOGF(
        DBG5,
        "No mount points currently configured, skipping accidental unmount recovery.");
    return;
  }

  const auto mountPoints = mountPoints_->rlock();
  for (const auto& client : dirs.items()) {
    auto mountPath = canonicalPath(client.first.stringPiece());
    const auto it = mountPoints->find(mountPath);

    if (it == mountPoints->end()) {
      // This mount point is not currently mounted, but it was configured
      // in config.json.  This means that the client was unmounted.
      // We should attempt to remount it, if it is unmounted accidentally.

      auto nfsServer = serverState_->getNfsServer();
      if (nfsServer && nfsServer->isMountRegistered(mountPath)) {
        // This mount point is registered with the NFS server, so it is
        // probably in the process of being unmounted.  We should not
        // attempt to remount it.
        continue;
      }

      auto edenClientPath =
          edenDir_.getCheckoutStateDir(client.second.stringPiece());
      auto initialConfig =
          CheckoutConfig::loadFromClientDirectory(mountPath, edenClientPath);
      if (initialConfig->isIntentionallyUnmounted()) {
        XLOGF(
            DBG5,
            "Mount point {} is intentionally unmounted, skipping unmount recovery.",
            mountPath);
        continue;
      }

      XLOGF(
          INFO,
          "Mount point {} is not currently mounted, attempting to remount.",
          mountPath);

      folly::via(
          getServerState()->getThreadPool().get(),
          [this,
           initialConfig = std::move(initialConfig),
           structuredLogger = structuredLogger_,
           mountPath,
           repoName = client.second.asString()]() mutable {
            return mount(std::move(initialConfig), /*readOnly=*/false)
                .thenTry([mountPath,
                          structuredLogger,
                          repoName = std::move(repoName)](
                             folly::Try<std::shared_ptr<EdenMount>>&& result) {
                  bool success = result.hasValue();
                  std::string exceptionMessage =
                      success ? "" : result.exception().what().toStdString();
                  structuredLogger->logEvent(AccidentalUnmountRecovery{
                      exceptionMessage, success, repoName});
                  if (success) {
                    XLOGF(
                        DBG3,
                        "Automatically remounted mount: {}",
                        mountPath.value());
                    return ImmediateFuture<folly::Unit>{std::in_place};
                  } else {
                    auto errorMessage = fmt::format(
                        "Failed to remount {}: {}\n",
                        mountPath.value(),
                        result.exception().what());
                    XLOG(ERR, errorMessage);
                    return makeImmediateFuture<Unit>(
                        std::move(result).exception());
                  }
                })
                .semi();
          });
    }
  }
}

void EdenServer::detectNfsCrawl() {
  auto edenConfig = config_->getEdenConfig();
  auto readThreshold = edenConfig->nfsCrawlReadThreshold.getValue();
  auto readDirThreshold = edenConfig->nfsCrawlReadDirThreshold.getValue();
  auto readCount = fb303::ServiceData::get()
                       ->getCounterIfExists(kNfsReadCount60)
                       .value_or(0);
  // At runtime, use-readdirplus can change, but this does not change the state
  // of the current mounts. Therefore, sum readdir and readdirplus counters and
  // compare that to the readdir threshold.
  auto readDirCount = fb303::ServiceData::get()
                          ->getCounterIfExists(kNfsReadDirCount60)
                          .value_or(0) +
      fb303::ServiceData::get()
          ->getCounterIfExists(kNfsReadDirPlusCount60)
          .value_or(0);
  if (readCount > readThreshold || readDirCount > readDirThreshold) {
    XLOGF(
        DBG2,
        "NFS crawl detected, initiating process discovery and attribution: "
        "[nfs.read_us.count.60 = {} > {} or nfs.readdir[plus]_us.count.60 = {} > {}]",
        readCount,
        readThreshold,
        readDirCount,
        readDirThreshold);

    // Get list of excluded process names
    auto exclusions = edenConfig->nfsCrawlExcludedProcessNames.getValue();

    // Iterate over each mount
    auto mountPoints = getMountPoints();
    for (auto& mountPointHandle : mountPoints) {
      if (mountPointHandle.getEdenMount().isNfsdChannel()) {
        folly::via(
            getServerState()->getThreadPool().get(),
            [mountPointHandle,
             exclusions,
             serverState = serverState_,
             readCount,
             readThreshold,
             readDirCount,
             readDirThreshold]() {
              const auto& mount = mountPointHandle.getEdenMount();

              // Information about the processes we've observed accessing the
              // mount and their parents. This represents a subtree of the
              // processes running at the time of crawl detection with edges
              // indicated by the ppid field. The init process (PID 1) is the
              // implicit root of the tree.
              std::unordered_map<pid_t, ProcessInfo> processRecords;

              // We'll keep track of PIDs known not to be leaves in our tree to
              // simplify traversal below.
              std::unordered_set<pid_t> nonLeafPids;

              // Get list of pids that have open files/paths on the mount
              auto mountPids =
                  proc_util::readProcessIdsForPath(mount.getPath());

              for (pid_t pid : mountPids) {
                // Walk up the process hierarchy from the PID seen accessing the
                // mount until we hit either init or a process we've already
                // recorded.
                while (pid > 1 &&
                       processRecords.find(pid) == processRecords.end()) {
                  auto processInfo =
                      serverState->getProcessInfoCache()->lookup(pid).get();
                  auto ppid = processInfo.ppid;
                  nonLeafPids.insert(ppid);
                  processRecords.insert({pid, std::move(processInfo)});
                  pid = ppid;
                }
              }

              // Iterate over leaf PIDs and walk up their process hierarchies
              // for logging.
              for (const auto& pair : processRecords) {
                auto pid = pair.first;
                auto record = pair.second;
                if (nonLeafPids.find(pid) != nonLeafPids.end() ||
                    exclusions.find(record.simpleName) != exclusions.end()) {
                  continue;
                }

                // Gather hierarchy into a stack so we can log in the order of
                // "parent -> child -> grandchild".
                std::vector<std::pair<pid_t, ProcessInfo>> hierarchy;
                decltype(processRecords)::iterator itr;
                while (pid > 1 &&
                       (itr = processRecords.find(pid)) !=
                           processRecords.end()) {
                  pid = itr->second.ppid;
                  hierarchy.emplace_back(pid, std::move(itr->second));
                }

                // Log process hierarchies
                auto [p, pi] = std::move(hierarchy.back());
                hierarchy.pop_back();
                std::string output =
                    fmt::format("[{}({}): {}]", pi.simpleName, p, pi.name);
                while (!hierarchy.empty()) {
                  fmt::format_to(std::back_inserter(output), " -> ");
                  auto [child_p, child_pi] = std::move(hierarchy.back());
                  hierarchy.pop_back();
                  fmt::format_to(
                      std::back_inserter(output),
                      "[{}({}): {}]",
                      child_pi.simpleName,
                      child_p,
                      child_pi.name);
                }
                XLOGF(
                    DBG2,
                    "NFS crawl detection found process with open files in mount point: {}\n  {}",
                    mount.getPath(),
                    output);
                serverState->getStructuredLogger()->logEvent(NfsCrawlDetected{
                    readCount,
                    readThreshold,
                    readDirCount,
                    readDirThreshold,
                    output});
              }
            });
      }
    }
  }
}

void EdenServer::reloadConfig() {
  // Get the config, forcing a reload now.
  auto config = serverState_->getReloadableConfig()->getEdenConfig(
      ConfigReloadBehavior::ForceReload);

  // Update all periodic tasks that are controlled by config settings.
  // This should be cheap, so for now we just block on this to finish rather
  // than returning a Future.  We could change this to return a Future if we
  // found a reason to do so in the future.
  mainEventBase_->runImmediatelyOrRunInEventBaseThreadAndWait(
      [this, config = std::move(config)] {
        updatePeriodicTaskIntervals(*config);
      });
}

void EdenServer::checkLockValidity() {
  if (edenDir_.isLockValid()) {
    return;
  }

  // Exit if our lock file no longer looks valid.
  // This ensures EdenFS process quits if someone deletes the `.eden` state
  // directory or moves it to another location.  Otherwise an EdenFS process
  // could continue running indefinitely in the background even though its state
  // directory no longer exists.
  XLOG(ERR, "Stopping EdenFS: on-disk lock file is no longer valid");

  // Attempt an orderly shutdown for now.  Since our state directory may have
  // been deleted we might not really be able to shut down normally, but for now
  // we'll try.  We potentially could try more aggressive measures (exit() or
  // _exit()) if we find that trying to stop normally here ever causes problems.
  stop();
}

apache::thrift::ServerStream<std::string>
EdenServer::createStartupStatusThriftStream() {
  return ThriftStreamStartupStatusSubscriber::createStartupStatusThriftStream(
      startupStatusChannel_);
}

void EdenServer::clearStartupStatusPublishers() {
  startupStatusChannel_->startupCompleted();
}

} // namespace facebook::eden
