/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#include <cpptoml.h>
#include <chrono>

#include <sys/stat.h>
#include <atomic>
#include <fstream>
#include <functional>
#include <memory>
#include <sstream>
#include <string>

#include <fb303/ServiceData.h>
#include <fmt/core.h>
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/io/async/HHWheelTimer.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include <folly/stop_watch.h>
#include <signal.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>
#include <thrift/lib/cpp2/server/ThriftProcessor.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "eden/common/utils/ProcessNameCache.h"
#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/config/TomlConfig.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/journal/Journal.h"
#include "eden/fs/nfs/NfsServer.h"
#include "eden/fs/notifications/NullNotifier.h"
#include "eden/fs/service/EdenCPUThreadPool.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/service/ThriftUtil.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BackingStoreLogger.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/LocalStoreCachedBackingStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/SqliteLocalStore.h"
#include "eden/fs/store/TreeCache.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/IHiveLogger.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/telemetry/StructuredLoggerFactory.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/EdenError.h"
#include "eden/fs/utils/EnumValue.h"
#include "eden/fs/utils/FileUtils.h"
#include "eden/fs/utils/FsChannelTypes.h"
#include "eden/fs/utils/NfsSocket.h"
#include "eden/fs/utils/NotImplemented.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/ProcUtil.h"
#include "eden/fs/utils/TimeUtil.h"
#include "eden/fs/utils/UserInfo.h"

#ifndef _WIN32
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/notifications/CommandNotifier.h"
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverServer.h"
#else
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
    unload_interval_minutes,
    0,
    "Frequency in minutes of background inode unloading");
DEFINE_int64(
    start_delay_minutes,
    10,
    "Initial delay before first background inode unload");
DEFINE_int64(
    unload_age_minutes,
    6 * 60,
    "Minimum age of the inodes to be unloaded in background");

DEFINE_uint64(
    maximumBlobCacheSize,
    40 * 1024 * 1024,
    "How many bytes worth of blobs to keep in memory, at most");
DEFINE_uint64(
    minimumBlobCacheEntryCount,
    16,
    "The minimum number of recent blobs to keep cached. Trumps maximumBlobCacheSize");

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
    return std::make_shared<WindowsNotifier>(
        config, version, std::chrono::steady_clock::now());
  } else {
    return std::make_shared<NullNotifier>(config);
  }
#else
  (void)version;
  return std::make_shared<CommandNotifier>(config);
#endif // _WIN32
}

constexpr StringPiece kRocksDBPath{"storage/rocks-db"};
constexpr StringPiece kSqlitePath{"storage/sqlite.db"};
constexpr StringPiece kHgStorePrefix{"store.hg"};
#ifndef _WIN32
constexpr StringPiece kFuseRequestPrefix{"fuse"};
#endif
constexpr StringPiece kStateConfig{"config.toml"};

std::optional<std::string> getUnixDomainSocketPath(
    const folly::SocketAddress& address) {
  return AF_UNIX == address.getFamily() ? std::make_optional(address.getPath())
                                        : std::nullopt;
}

std::string getCounterNameForImportMetric(
    RequestMetricsScope::RequestStage stage,
    RequestMetricsScope::RequestMetric metric,
    std::optional<HgBackingStore::HgImportObject> object = std::nullopt) {
  if (object.has_value()) {
    // base prefix . stage . object . metric
    return folly::to<std::string>(
        kHgStorePrefix,
        ".",
        RequestMetricsScope::stringOfHgImportStage(stage),
        ".",
        HgBackingStore::stringOfHgImportObject(object.value()),
        ".",
        RequestMetricsScope::stringOfRequestMetric(metric));
  }
  // base prefix . stage . metric
  return folly::to<std::string>(
      kHgStorePrefix,
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

} // namespace

namespace facebook::eden {

class EdenServer::ThriftServerEventHandler
    : public apache::thrift::server::TServerEventHandler,
      public folly::AsyncSignalHandler {
 public:
  explicit ThriftServerEventHandler(EdenServer* edenServer)
      : AsyncSignalHandler{nullptr}, edenServer_{edenServer} {}

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
    runningPromise_.setValue();
  }

  void signalReceived(int sig) noexcept override {
    // Stop the server.
    // Unregister for this signal first, so that we will be terminated
    // immediately if the signal is sent again before we finish stopping.
    // This makes it easier to kill the daemon if graceful shutdown hangs or
    // takes longer than expected for some reason.  (For instance, if we
    // unmounting the mount points hangs for some reason.)
    XLOG(INFO) << "stopping due to signal " << sig;
    unregisterSignalHandler(sig);
    edenServer_->stop();
  }

  /**
   * Return a Future that will be fulfilled once the thrift server is bound to
   * its socket and is ready to accept conenctions.
   */
  Future<Unit> getThriftRunningFuture() {
    return runningPromise_.getFuture();
  }

 private:
  EdenServer* edenServer_{nullptr};
  folly::Promise<Unit> runningPromise_;
};

static constexpr folly::StringPiece kBlobCacheMemory{"blob_cache.memory"};

EdenServer::EdenServer(
    std::vector<std::string> originalCommandLine,
    UserInfo userInfo,
    SessionInfo sessionInfo,
    std::unique_ptr<PrivHelper> privHelper,
    std::shared_ptr<const EdenConfig> edenConfig,
    ActivityRecorderFactory activityRecorderFactory,
    BackingStoreFactory* backingStoreFactory,
    std::shared_ptr<IHiveLogger> hiveLogger,
    std::string version)
    : originalCommandLine_{std::move(originalCommandLine)},
      edenDir_{edenConfig->edenDir.getValue()},
      activityRecorderFactory_{std::move(activityRecorderFactory)},
      backingStoreFactory_{backingStoreFactory},
      blobCache_{BlobCache::create(
          FLAGS_maximumBlobCacheSize,
          FLAGS_minimumBlobCacheEntryCount)},
      config_{std::make_shared<ReloadableConfig>(edenConfig)},
      mountPoints_{std::make_shared<folly::Synchronized<MountMap>>(
          MountMap{kPathMapDefaultCaseSensitive})},
      // Store a pointer to the EventBase that will be used to drive
      // the main thread.  The runServer() code will end up driving this
      // EventBase.
      mainEventBase_{folly::EventBaseManager::get()->getEventBase()},
      serverState_{make_shared<ServerState>(
          std::move(userInfo),
          std::move(privHelper),
          std::make_shared<EdenCPUThreadPool>(),
          std::make_shared<UnixClock>(),
          std::make_shared<ProcessNameCache>(),
          makeDefaultStructuredLogger(*edenConfig, std::move(sessionInfo)),
          std::move(hiveLogger),
          config_,
          *edenConfig,
          mainEventBase_,
          getPlatformNotifier(config_, version),
          FLAGS_enable_fault_injection)},
      version_{std::move(version)},
      progressManager_{std::make_unique<
          folly::Synchronized<EdenServer::ProgressManager>>()} {
  treeCache_ = TreeCache::create(serverState_->getReloadableConfig());
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->registerCallback(kBlobCacheMemory, [this] {
    return this->getBlobCache()->getStats().totalSizeInBytes;
  });

  registerInodePopulationReportsCallback();

  for (auto stage : RequestMetricsScope::requestStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : HgBackingStore::hgImportObjects) {
        auto counterName = getCounterNameForImportMetric(stage, metric, object);
        counters->registerCallback(counterName, [this, stage, object, metric] {
          auto individual_counters = this->collectHgQueuedBackingStoreCounters(
              [stage, object, metric](const HgQueuedBackingStore& store) {
                return store.getImportMetric(stage, object, metric);
              });
          return RequestMetricsScope::aggregateMetricCounters(
              metric, individual_counters);
        });
      }
      auto summaryCounterName = getCounterNameForImportMetric(stage, metric);
      counters->registerCallback(summaryCounterName, [this, stage, metric] {
        std::vector<size_t> individual_counters;
        for (auto object : HgBackingStore::hgImportObjects) {
          auto more_counters = this->collectHgQueuedBackingStoreCounters(
              [stage, object, metric](const HgQueuedBackingStore& store) {
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
}

EdenServer::~EdenServer() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(kBlobCacheMemory);

  unregisterInodePopulationReportsCallback();

  for (auto stage : RequestMetricsScope::requestStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : HgBackingStore::hgImportObjects) {
        auto counterName = getCounterNameForImportMetric(stage, metric, object);
        counters->unregisterCallback(counterName);
      }
      auto summaryCounterName = getCounterNameForImportMetric(stage, metric);
      counters->unregisterCallback(summaryCounterName);
    }
  }
}

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

void EdenServer::ProgressManager::printProgresses(
    std::shared_ptr<StartupLogger> logger) {
  std::string prepare;
  std::string content;

  if (totalLinesPrinted) {
    prepare = cursor_helper::move_cursor_up(totalLinesPrinted);
    totalLinesPrinted = 0;
  }
  prepare += cursor_helper::clear_to_bottom();

  unsigned int printedFinished = 0;
  unsigned int printedInProgress = 0;
  for (auto& it : progresses) {
    if (it.mountFinished) {
      content +=
          folly::to<std::string>("Successfully remounted ", it.mountPath, "\n");
      printedFinished++;
    } else if (!it.fsckStarted) {
      content += folly::to<std::string>("Remounting ", it.mountPath, "\n");
      printedFinished++;
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
      content += folly::to<std::string>(
          "and ",
          totalFinished - printedFinished,
          " finished, ",
          totalInProgress - printedInProgress,
          " in progress...");
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
  progresses.emplace_back(
      ProgressState(std::move(mountPath), std::move(localDir)));
  totalInProgress++;
  return progressIndex;
}

Future<Unit> EdenServer::unmountAll() {
  std::vector<Future<Unit>> futures;
  {
    const auto mountPoints = mountPoints_->wlock();
    for (auto& entry : *mountPoints) {
      auto& info = entry.second;

      // Note: capturing the shared_ptr<EdenMount> here in the thenTry() lambda
      // is important to ensure that the EdenMount object cannot be destroyed
      // before EdenMount::unmount() completes.
      auto mount = info.edenMount;
      auto future = mount->unmount().thenTry(
          [mount, unmountFuture = info.unmountPromise.getFuture()](
              auto&& result) mutable {
            if (result.hasValue()) {
              return std::move(unmountFuture);
            } else {
              XLOG(ERR) << "Failed to perform unmount for \""
                        << mount->getPath()
                        << "\": " << folly::exceptionStr(result.exception());
              return makeFuture<Unit>(result.exception());
            }
          });
      futures.push_back(std::move(future));
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).toUnsafeFuture().thenValue(
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
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      auto& info = entry.second;

      try {
        info.takeoverPromise.emplace();
        auto future = info.takeoverPromise->getFuture();

        if (auto* channel = info.edenMount->getFuseChannel()) {
          XLOG(DBG7) << "Calling takeover stop on fuse channel";
          channel->takeoverStop();
        } else if (auto* channel = info.edenMount->getNfsdChannel()) {
          XLOG(DBG7) << "Calling takeover stop on nfsd3";
          channel->takeoverStop();
        } else if (!info.edenMount->fsChannelIsInitialized()) {
          return EDEN_BUG_FUTURE(TakeoverData)
              << "Takeover isn't (yet) supported during mount initialization."
              << "Mount State "
              << folly::to_underlying(info.edenMount->getState());
        } else {
          auto mountProtocol = info.edenMount->getMountProtocol();
          std::string formatedMountProtocol = "<unknown>";
          if (mountProtocol.has_value()) {
            formatedMountProtocol =
                fmt::format("{}", folly::to_underlying(mountProtocol.value()));
          }
          return EDEN_BUG_FUTURE(TakeoverData)
              << "Takeover isn't (yet) supported for non-FUSE/NFS mounts."
              << "Mount type: " << formatedMountProtocol << ". Mount State: "
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
        XLOG(ERR) << "Error while stopping \"" << mountPath
                  << "\" for takeover: " << ew;
        futures.push_back(
            makeFuture<optional<TakeoverData::MountInfo>>(std::move(ew)));
      }
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).toUnsafeFuture().thenValue(
      [takeoverPromise = std::move(takeoverPromise)](
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
            XLOG(ERR) << "error stopping mount during takeover shutdown: "
                      << result.exception().what();
            continue;
          }

          // result might be a successful Try with an empty Optional.
          // This could happen if the mount point was unmounted while we were
          // in the middle of stopping it for takeover.  Just skip this mount
          // in this case.
          if (!result.value().has_value()) {
            XLOG(WARN) << "mount point was unmounted during "
                          "takeover shutdown";
            continue;
          }

          data.mountPoints.emplace_back(std::move(result.value().value()));
        }
        return data;
      });
}
#endif

void EdenServer::startPeriodicTasks() {
  // Report memory usage stats once every 30 seconds
  memoryStatsTask_.updateInterval(30s);
  auto config = serverState_->getReloadableConfig()->getEdenConfig();
  updatePeriodicTaskIntervals(*config);

#ifndef _WIN32
  // Schedule a periodic job to unload unused inodes based on the last access
  // time. currently Eden does not have accurate timestamp tracking for inodes,
  // so using unloadChildrenNow just to validate the behaviour. We will have to
  // modify current unloadChildrenNow function to unload inodes based on the
  // last access time.
  if (FLAGS_unload_interval_minutes > 0) {
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

  if (config.enableGc.getValue()) {
    gcTask_.updateInterval(
        std::chrono::duration_cast<std::chrono::milliseconds>(
            config.gcPeriod.getValue()));
  }
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
      XLOG(DBG3) << "Callback expired, running function";
      try {
        fn_();
      } catch (std::exception const& e) {
        LOG(ERROR) << "HHWheelTimerBase timeout callback threw an exception: "
                   << e.what();
      } catch (...) {
        LOG(ERROR)
            << "HHWheelTimerBase timeout callback threw a non-exception.";
      }
      // We have to use a CallBack* to schedule. We need to destroy ourselves
      // because no one else will
      delete this;
    }
    // The callback will be canceled if the timer is destroyed. Or we use
    // deduplication We don't want to run fn during destruction.
    void callbackCanceled() noexcept override {
      XLOG(DBG3) << "Callback cancled, NOT running function";
      // We have to use a CallBack* to schedule. We need to destory ourselves
      // because no one else will
      delete this;
    }
    std::function<void()> fn_;
  };

  // I hate using raw pointers, but we have to to schedule the callback.
  Wrapper* w = new Wrapper(std::move(fn));
  // Note callback will be run in the mainEventBase_ Thread.
  // TODO: would be nice to deduplicate these function calls. We need the
  // same type of callback to use the same wrapper object ... singletons?
  // The main difference with scheduleTimeoutFn is whether the callback is
  // invoked when the EventBase is destroyed: scheduleTimeoutFn will call it,
  // this function won't. See comment above the wrapper class.
  mainEventBase_->timer().scheduleTimeout(w, timeout);
}

#ifndef _WIN32
void EdenServer::unloadInodes() {
  struct Root {
    AbsolutePath mountName;
    TreeInodePtr rootInode;
    shared_ptr<EdenMount> mount;
  };
  std::vector<Root> roots;
  {
    const auto mountPoints = mountPoints_->wlock();
    for (auto& entry : *mountPoints) {
      roots.emplace_back(Root{
          entry.first,
          entry.second.edenMount->getRootInode(),
          entry.second.edenMount});
    }
  }

  if (!roots.empty()) {
    auto cutoff = std::chrono::system_clock::now() -
        std::chrono::minutes(FLAGS_unload_age_minutes);
    auto cutoff_ts = folly::to<timespec>(cutoff);
    for (auto& [name, rootInode, mount] : roots) {
      auto unloaded = rootInode->unloadChildrenLastAccessedBefore(cutoff_ts);
      if (unloaded) {
        XLOG(INFO) << "Unloaded " << unloaded
                   << " inodes in background from mount " << name;
      }
      mount->getInodeMap()->recordPeriodicInodeUnload(unloaded);
    }
  }

  scheduleInodeUnload(std::chrono::minutes(FLAGS_unload_interval_minutes));
}

void EdenServer::scheduleInodeUnload(std::chrono::milliseconds timeout) {
  mainEventBase_->timer().scheduleTimeoutFn(
      [this] {
        XLOG(DBG4) << "Beginning periodic inode unload";
        unloadInodes();
      },
      timeout);
}

Future<Unit> EdenServer::recover(TakeoverData&& data) {
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

Future<Unit> EdenServer::recoverImpl(TakeoverData&& takeoverData) {
  auto thriftRunningFuture = createThriftServer();

  const auto takeoverPath = edenDir_.getTakeoverSocketPath();

  // Recover the eden lock file and the thrift server socket.
  edenDir_.takeoverLock(std::move(takeoverData.lockFile));
  server_->useExistingSocket(takeoverData.thriftSocket.release());

  // Remount our mounts from our prepared takeoverData
  std::vector<Future<Unit>> mountFutures;
  mountFutures = prepareMountsTakeover(
      std::make_unique<ForegroundStartupLogger>(),
      std::move(takeoverData.mountPoints));

  // Return a future that will complete only when all mount points have
  // started and the thrift server is also running.
  mountFutures.emplace_back(std::move(thriftRunningFuture));
  return folly::collectAllUnsafe(mountFutures).unit();
}

#endif // !_WIN32

void EdenServer::serve() const {
  getServer()->serve();
}

Future<Unit> EdenServer::prepare(std::shared_ptr<StartupLogger> logger) {
  return prepareImpl(std::move(logger))
      .ensure(
          // Mark the server state as RUNNING once we finish setting up the
          // mount points. Even if an error occurs we still transition to the
          // running state. The prepare() code will log an error with more
          // details if we do fail to set up some of the mount points.
          [this] { runningState_.wlock()->state = RunState::RUNNING; });
}

Future<Unit> EdenServer::prepareImpl(std::shared_ptr<StartupLogger> logger) {
  bool doingTakeover = false;
  if (!edenDir_.acquireLock()) {
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
#ifndef _WIN32
  // Start the PrivHelper client, using our main event base to drive its I/O
  serverState_->getPrivHelper()->attachEventBase(mainEventBase_);
#endif

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
    takeoverData = takeoverMounts(takeoverPath);
    logger->log(
        "Received takeover information for ",
        takeoverData.mountPoints.size(),
        " mount points");

    // Take over the eden lock file and the thrift server socket.
    edenDir_.takeoverLock(std::move(takeoverData.lockFile));
    server_->useExistingSocket(takeoverData.thriftSocket.release());
#else
    NOT_IMPLEMENTED();
#endif // !_WIN32
  } else {
    // Remove any old thrift socket from a previous (now dead) edenfs daemon.
    prepareThriftAddress();
  }

#ifndef _WIN32
  if (auto nfsServer = serverState_->getNfsServer()) {
    if (doingTakeover && takeoverData.mountdServerSocket.has_value()) {
      XLOG(DBG7) << "Initializing mountd from existing socket";
      nfsServer->initialize(std::move(takeoverData.mountdServerSocket.value()));
    } else {
      XLOG(DBG7) << "Initializing mountd from scratch";
      std::optional<AbsolutePath> unixSocketPath;
      if (serverState_->getEdenConfig()->useUnixSocket.getValue()) {
        unixSocketPath = edenDir_.getMountdSocketPath();
      }
      nfsServer->initialize(
          makeNfsSocket(std::move(unixSocketPath)),
          serverState_->getEdenConfig()->registerMountd.getValue());
    }
  }
#endif

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
        openStorageEngine(*logger);

        std::vector<Future<Unit>> mountFutures;
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
        return folly::collectAllUnsafe(std::move(mountFutures)).unit();
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
    XLOG(DBG2) << "Creating new memory store.";
    localStore_ = make_shared<MemoryLocalStore>();
  } else if (storageEngine == "sqlite") {
    const auto path = edenDir_.getPath() + RelativePathPiece{kSqlitePath};
    const auto parentDir = path.dirname();
    ensureDirectoryExists(parentDir);
    XLOG(DBG2) << "Creating local SQLite store " << path << "...";
    folly::stop_watch<std::chrono::milliseconds> watch;
    localStore_ = make_shared<SqliteLocalStore>(path);
    XLOG(DBG2) << "Opened SQLite store in " << watch.elapsed().count() / 1000.0
               << " seconds.";
  } else if (storageEngine == "rocksdb") {
    XLOG(DBG2) << "Creating local RocksDB store...";
    folly::stop_watch<std::chrono::milliseconds> watch;
    const auto rocksPath = edenDir_.getPath() + RelativePathPiece{kRocksDBPath};
    ensureDirectoryExists(rocksPath);
    localStore_ = make_shared<RocksDbLocalStore>(
        rocksPath,
        serverState_->getStructuredLogger(),
        &serverState_->getFaultInjector());
    localStore_->enableBlobCaching.store(
        serverState_->getEdenConfig()->enableBlobCaching.getValue(),
        std::memory_order_relaxed);
    XLOG(DBG2) << "Created RocksDB store in "
               << watch.elapsed().count() / 1000.0 << " seconds.";
  } else {
    throw std::runtime_error(
        folly::to<string>("invalid storage engine: ", storageEngine));
  }

  return configUpdated;
}

void EdenServer::openStorageEngine(StartupLogger& logger) {
  logger.log("Opening local store...");
  folly::stop_watch<std::chrono::milliseconds> watch;
  localStore_->open();
  logger.log(
      "Opened local store in ", watch.elapsed().count() / 1000.0, " seconds.");
}

std::vector<Future<Unit>> EdenServer::prepareMountsTakeover(
    shared_ptr<StartupLogger> logger,
    std::vector<TakeoverData::MountInfo>&& takeoverMounts) {
  // Trigger remounting of existing mount points
  // If doingTakeover is true, use the mounts received in TakeoverData
  std::vector<Future<Unit>> mountFutures;

  if (folly::kIsWindows) {
    NOT_IMPLEMENTED();
  }

  for (auto& info : takeoverMounts) {
    const auto stateDirectory = info.stateDirectory;
    auto mountFuture =
        makeFutureWith([&] {
          auto initialConfig = CheckoutConfig::loadFromClientDirectory(
              AbsolutePathPiece{info.mountPath},
              AbsolutePathPiece{info.stateDirectory});

          return mount(
              std::move(initialConfig), false, [](auto) {}, std::move(info));
        })
            .thenTry([logger, mountPath = info.mountPath](
                         folly::Try<std::shared_ptr<EdenMount>>&& result) {
              if (result.hasValue()) {
                logger->log("Successfully took over mount ", mountPath);
                return makeFuture();
              } else {
                incrementStartupMountFailures();
                logger->warn(
                    "Failed to perform takeover for ",
                    mountPath,
                    ": ",
                    result.exception().what());
                return makeFuture<Unit>(std::move(result).exception());
              }
            });
    mountFutures.push_back(std::move(mountFuture));
  }

  return mountFutures;
}

std::vector<Future<Unit>> EdenServer::prepareMounts(
    shared_ptr<StartupLogger> logger) {
  std::vector<Future<Unit>> mountFutures;
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
    auto mountFuture = makeFutureWith([&] {
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
              return makeFuture();
            } else {
              incrementStartupMountFailures();
              logger->warn(
                  "Failed to remount ",
                  mountPath,
                  ": ",
                  result.exception().what());
              return makeFuture<Unit>(std::move(result).exception());
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
#ifndef _WIN32
  folly::stop_watch<> shutdown;
  bool shutdownSuccess = true;
  SCOPE_EXIT {
    auto shutdownTimeInSeconds =
        std::chrono::duration<double>{shutdown.elapsed()}.count();
    serverState_->getStructuredLogger()->logEvent(
        DaemonStop{shutdownTimeInSeconds, takeover, shutdownSuccess});
  };
#endif
  auto&& shutdownFuture =
      folly::Future<std::optional<TakeoverData>>::makeEmpty();
  {
    auto state = runningState_.wlock();
    takeover = state->shutdownFuture.valid();
    if (takeover) {
      shutdownFuture = std::move(state->shutdownFuture);
    }
    XDCHECK_EQ(state->state, RunState::SHUTTING_DOWN);
    state->state = RunState::SHUTTING_DOWN;
  }
  if (!takeover) {
    shutdownFuture =
        performNormalShutdown().thenValue([](auto&&) { return std::nullopt; });
  }

  XDCHECK(shutdownFuture.valid())
      << "shutdownFuture should not be empty during cleanup";

  // Drive the main event base until shutdownFuture completes
  XCHECK_EQ(mainEventBase_, folly::EventBaseManager::get()->getEventBase());
  while (!shutdownFuture.isReady()) {
    mainEventBase_->loopOnce();
  }
  auto&& shutdownResult = shutdownFuture.result();
#ifndef _WIN32
  shutdownSuccess = !shutdownResult.hasException();

  // We must check if the shutdownResult contains TakeoverData, and if so
  // we must recover
  if (shutdownResult.hasValue()) {
    auto&& shutdownValue = shutdownResult.value();
    if (shutdownValue.has_value()) {
      // shutdownValue only contains a value if a takeover was not successful.
      shutdownSuccess = false;
      XLOG(INFO)
          << "edenfs encountered a takeover error, attempting to recover";
      // We do not wait here for the remounts to succeed, and instead will
      // let runServer() drive the mainEventBase loop to finish this call
      (void)recover(std::move(shutdownValue).value());
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

Future<Unit> EdenServer::performNormalShutdown() {
#ifndef _WIN32
  takeoverServer_.reset();
#endif // !_WIN32

  // Clean up all the server mount points
  return unmountAll();
}

void EdenServer::shutdownPrivhelper() {
#ifndef _WIN32
  // Explicitly stop the privhelper process so we can verify that it
  // exits normally.
  const auto privhelperExitCode = serverState_->getPrivHelper()->stop();
  if (privhelperExitCode != 0) {
    if (privhelperExitCode > 0) {
      XLOG(ERR) << "privhelper process exited with unexpected code "
                << privhelperExitCode;
    } else {
      XLOG(ERR) << "privhelper process was killed by signal "
                << privhelperExitCode;
    }
  }
#endif
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

folly::Future<folly::Unit> EdenServer::performFreshStart(
    std::shared_ptr<EdenMount> edenMount,
    bool readOnly) {
  // Start up the fuse workers.
  return edenMount->startChannel(readOnly);
}

Future<Unit> EdenServer::performTakeoverStart(
    FOLLY_MAYBE_UNUSED std::shared_ptr<EdenMount> edenMount,
    FOLLY_MAYBE_UNUSED TakeoverData::MountInfo&& info) {
#ifndef _WIN32
  auto mountPath = info.mountPath;
  std::vector<std::string> bindMounts;
  for (const auto& bindMount : info.bindMounts) {
    bindMounts.emplace_back(bindMount.value());
  }

  auto future = completeTakeoverStart(edenMount, std::move(info));
  return std::move(future).thenValue(
      [this,
       edenMount = std::move(edenMount),
       bindMounts = std::move(bindMounts),
       mountPath = std::move(mountPath)](auto&&) mutable {
        return serverState_->getPrivHelper()->takeoverStartup(
            mountPath.view(), bindMounts);
      });
#else
  NOT_IMPLEMENTED();
#endif
}

Future<Unit> EdenServer::completeTakeoverStart(
    FOLLY_MAYBE_UNUSED std::shared_ptr<EdenMount> edenMount,
    FOLLY_MAYBE_UNUSED TakeoverData::MountInfo&& info) {
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

BackingStoreType toBackingStoreType(const std::string& type) {
  if (type == "git") {
    return BackingStoreType::GIT;
  } else if (type == "hg") {
    return BackingStoreType::HG;
  } else if (type == "recas") {
    return BackingStoreType::RECAS;
  } else if (type == "http") {
    return BackingStoreType::HTTP;
  } else if (type == "") {
    return BackingStoreType::EMPTY;
  } else {
    throw std::domain_error(
        folly::to<std::string>("unsupported backing store type: ", type));
  }
}

folly::Future<std::shared_ptr<EdenMount>> EdenServer::mount(
    std::unique_ptr<CheckoutConfig> initialConfig,
    bool readOnly,
    OverlayChecker::ProgressCallback&& progressCallback,
    optional<TakeoverData::MountInfo>&& optionalTakeover) {
  folly::stop_watch<> mountStopWatch;

  auto backingStore = getBackingStore(
      toBackingStoreType(initialConfig->getRepoType()),
      initialConfig->getRepoSource(),
      *initialConfig);

  auto objectStore = ObjectStore::create(
      getLocalStore(),
      backingStore,
      treeCache_,
      getSharedStats(),
      serverState_->getProcessNameCache(),
      serverState_->getStructuredLogger(),
      serverState_->getReloadableConfig()->getEdenConfig(),
      initialConfig->getCaseSensitive());
  auto journal = std::make_unique<Journal>(getSharedStats());

  // Create the EdenMount object and insert the mount into the mountPoints_ map.
  auto edenMount = EdenMount::create(
      std::move(initialConfig),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal));
  addToMountPoints(edenMount);

  registerStats(edenMount);

  const bool doTakeover = optionalTakeover.has_value();

  auto initFuture = edenMount->initialize(
      std::move(progressCallback),
      doTakeover ? std::make_optional(optionalTakeover->inodeMap)
                 : std::nullopt);

  // Now actually begin starting the mount point
  return std::move(initFuture)
      .thenTry([this,
                doTakeover,
                readOnly,
                edenMount,
                mountStopWatch,
                optionalTakeover = std::move(optionalTakeover)](
                   folly::Try<Unit>&& result) mutable {
        if (result.hasException()) {
          XLOG(ERR) << "error initializing " << edenMount->getPath() << ": "
                    << result.exception().what();
          mountFinished(edenMount.get(), std::nullopt);
          return makeFuture<shared_ptr<EdenMount>>(
              std::move(result).exception());
        }
        return (optionalTakeover ? performTakeoverStart(
                                       edenMount, std::move(*optionalTakeover))
                                 : performFreshStart(edenMount, readOnly))
            .thenTry([edenMount, doTakeover, this](
                         folly::Try<Unit>&& result) mutable {
              // Call mountFinished() if an error occurred during FUSE
              // initialization.
              if (result.hasException()) {
                mountFinished(edenMount.get(), std::nullopt);
                return makeFuture<shared_ptr<EdenMount>>(
                    std::move(result).exception());
              }

              registerStats(edenMount);

              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              auto finishFuture =
                  edenMount->getChannelCompletionFuture().thenTry(
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
                      XLOG(ERR)
                          << "Error while performing bind mounts, will continue with mount anyway: "
                          << folly::exceptionStr(ew);
                      return edenMount;
                    })
                    .via(getServerState()->getThreadPool().get());
              }
            })
            .thenTry([this, mountStopWatch, doTakeover, edenMount](auto&& t) {
              FinishedMount event;
              event.repo_type = edenMount->getCheckoutConfig()->getRepoType();
              event.repo_source =
                  basename(edenMount->getCheckoutConfig()->getRepoSource());
              if (auto mountProtocol = edenMount->getMountProtocol()) {
                event.fs_channel_type =
                    FieldConverter<MountProtocol>{}.toDebugString(
                        mountProtocol.value());
              } else {
                event.fs_channel_type = "unknown";
              }
              event.is_takeover = doTakeover;
              event.duration =
                  std::chrono::duration<double>{mountStopWatch.elapsed()}
                      .count();
              event.success = !t.hasException();
              event.clean = edenMount->getOverlay()->hadCleanStartup();
              serverState_->getStructuredLogger()->logEvent(event);
              return makeFuture(std::move(t));
            });
      });
}

Future<Unit> EdenServer::unmount(AbsolutePathPiece mountPath) {
  return makeFutureWith([&] {
           auto future = Future<Unit>::makeEmpty();
           auto mount = std::shared_ptr<EdenMount>{};
           {
             const auto mountPoints = mountPoints_->wlock();
             const auto it = mountPoints->find(mountPath);
             if (it == mountPoints->end()) {
               return makeFuture<Unit>(std::out_of_range(
                   fmt::format("no such mount point {}", mountPath)));
             }
             future = it->second.unmountPromise.getFuture();
             mount = it->second.edenMount;
           }

           // We capture the mount shared_ptr in the lambda to keep the
           // EdenMount object alive during the call to unmount.
           return mount->unmount().thenValue(
               [mount, f = std::move(future)](auto&&) mutable {
                 return std::move(f);
               });
         })
      .thenError([path = mountPath.copy()](folly::exception_wrapper&& ew) {
        XLOG(ERR) << "Failed to perform unmount for \"" << path
                  << "\": " << folly::exceptionStr(ew);
        return makeFuture<Unit>(std::move(ew));
      });
}

void EdenServer::mountFinished(
    EdenMount* edenMount,
    std::optional<TakeoverData::MountInfo> takeover) {
  const auto& mountPath = edenMount->getPath();
  XLOG(INFO) << "mount point \"" << mountPath << "\" stopped";

  // Save the unmount and takover Promises
  folly::SharedPromise<Unit> unmountPromise;
  std::optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;
  {
    const auto mountPoints = mountPoints_->wlock();
    const auto it = mountPoints->find(mountPath);
    XCHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    takeoverPromise = std::move(it->second.takeoverPromise);
  }

  const bool doTakeover = takeoverPromise.has_value();

  // Shutdown the EdenMount, and fulfill the unmount promise
  // when the shutdown completes
  edenMount->shutdown(doTakeover)
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

EdenServer::MountList EdenServer::getMountPoints() const {
  MountList results;
  {
    const auto mountPoints = mountPoints_->rlock();
    for (const auto& entry : *mountPoints) {
      const auto& mount = entry.second.edenMount;
      // Avoid returning mount points that are still initializing and are
      // not ready to perform inode operations yet.
      if (!mount->isSafeForInodeAccess()) {
        continue;
      }
      results.emplace_back(mount);
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

shared_ptr<EdenMount> EdenServer::getMount(AbsolutePathPiece mountPath) const {
  const auto mount = getMountUnsafe(mountPath);
  if (!mount->isSafeForInodeAccess()) {
    throw newEdenError(
        EBUSY,
        EdenErrorType::POSIX_ERROR,
        fmt::format("mount point \"{}\" is still initializing", mountPath));
  }
  return mount;
}

shared_ptr<EdenMount> EdenServer::getMountUnsafe(
    AbsolutePathPiece mountPath) const {
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
  return it->second.edenMount;
}

Future<CheckoutResult> EdenServer::checkOutRevision(
    AbsolutePathPiece mountPath,
    std::string& rootHash,
    std::optional<folly::StringPiece> rootHgManifest,
    std::optional<pid_t> clientPid,
    StringPiece callerName,
    CheckoutMode checkoutMode) {
  auto edenMount = getMount(mountPath);
  auto rootId = edenMount->getObjectStore()->parseRootId(rootHash);
  if (rootHgManifest.has_value()) {
    // The hg client has told us what the root manifest is.
    //
    // This is useful when a commit has just been created.  We won't be able to
    // ask the import helper to map the commit to its root manifest because it
    // won't know about the new commit until it reopens the repo.  Instead,
    // import the manifest for this commit directly.
    auto rootManifest = hash20FromThrift(rootHgManifest.value());
    edenMount->getObjectStore()
        ->getBackingStore()
        ->importManifestForRoot(rootId, rootManifest)
        .get();
  }
  // the +1 is so we count the current checkout that hasn't quite started yet
  getServerState()->getNotifier()->signalCheckout(
      enumerateInProgressCheckouts() + 1);
  return edenMount->checkout(rootId, clientPid, callerName, checkoutMode)
      .via(mainEventBase_)
      .thenValue([this, checkoutMode, edenMount, mountPath = mountPath.copy()](
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
        if (edenMount->isNfsdChannel() &&
            serverState_->getReloadableConfig()
                ->getEdenConfig()
                ->unloadUnlinkedInodes.getValue()) {
          // During whole Eden Process shutdown, this function can only be run
          // before the mount is destroyed.
          // This is because the function is either run before the server
          // event base is destroyed or it is not run at all, and the server
          // event base is destroyed before the mountPoints. Since the function
          // must be run before the eventbase is destroyed and the eventbase is
          // destroyed before the mountPoints, this function can only be called
          // before the mount points are destroyed during normal destruction.
          // However, the mount pont might have been unmounted before this
          // function is run outside of shutdown.
          auto delay = serverState_->getReloadableConfig()
                           ->getEdenConfig()
                           ->postCheckoutDelayToUnloadInodes.getValue();
          XLOG(DBG9) << "Scheduling unlinked inode cleanup for mount "
                     << mountPath << " in " << durationStr(delay)
                     << " seconds.";
          this->scheduleCallbackOnMainEventBase(
              std::chrono::duration_cast<std::chrono::milliseconds>(delay),
              [this, mountPath = mountPath.copy()]() {
                try {
                  auto edenMount = this->getMount(mountPath);
                  edenMount->forgetStaleInodes();
                } catch (EdenError& err) {
                  // This is an expected error if the mount has been
                  // unmounted before this callback ran.
                  if (err.errorCode_ref() == ENOENT) {
                    XLOG(DBG3)
                        << "Callback to clear inodes: Mount cannot be found. "
                        << (*err.message());
                  } else {
                    throw;
                  }
                }
              });
        }
        return std::move(result);
      })
      .via(getServerState()->getThreadPool().get());
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
          name, serverState_.get(), localStore_, getSharedStats(), config});
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

std::unordered_set<shared_ptr<HgQueuedBackingStore>>
EdenServer::getHgQueuedBackingStores() {
  std::unordered_set<std::shared_ptr<HgQueuedBackingStore>> hgBackingStores{};
  {
    auto lockedStores = this->backingStores_.rlock();
    for (auto entry : *lockedStores) {
      // TODO: remove these dynamic casts in favor of a QueryInterface method
      if (auto store =
              std::dynamic_pointer_cast<HgQueuedBackingStore>(entry.second)) {
        hgBackingStores.emplace(std::move(store));
      } else if (
          auto store = std::dynamic_pointer_cast<LocalStoreCachedBackingStore>(
              entry.second)) {
        auto inner_store = std::dynamic_pointer_cast<HgQueuedBackingStore>(
            store->getBackingStore());
        if (inner_store) {
          // dynamic_pointer_cast returns a copy of the shared pointer, so it is
          // safe to be moved
          hgBackingStores.emplace(std::move(inner_store));
        }
      }
    }
  }
  return hgBackingStores;
}

std::vector<size_t> EdenServer::collectHgQueuedBackingStoreCounters(
    std::function<size_t(const HgQueuedBackingStore&)> getCounterFromStore) {
  std::vector<size_t> counters;
  for (const auto& store : this->getHgQueuedBackingStores()) {
    counters.emplace_back(getCounterFromStore(*store));
  }
  return counters;
}

Future<Unit> EdenServer::createThriftServer() {
  auto edenConfig = config_->getEdenConfig();
  server_ = make_shared<ThriftServer>();
  server_->setMaxRequests(edenConfig->thriftMaxRequests.getValue());
  server_->setNumCPUWorkerThreads(edenConfig->thriftNumWorkers.getValue());
  server_->setCPUWorkerThreadName("Thrift-");
  server_->setQueueTimeout(std::chrono::floor<std::chrono::milliseconds>(
      edenConfig->thriftQueueTimeout.getValue()));
  server_->setAllowCheckUnimplementedExtraInterfaces(false);

  // Setting this allows us to to only do stopListening() on the stop() call
  // and delay thread-pool join (stop cpu workers + stop workers) untill
  // server object destruction. This specifically matters in the takeover
  // shutdown code path.
  server_->setStopWorkersOnStopListening(false);
  server_->leakOutstandingRequestsWhenServerStops(true);

  handler_ = make_shared<EdenServiceHandler>(originalCommandLine_, this);
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

#ifdef _WIN32
  auto constexpr maxTries = 3;
#else
  auto constexpr maxTries = 1;
#endif

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
      XLOG(INFO) << "stop was called while server was already shutting down";
      return;
    }
    state->state = RunState::SHUTTING_DOWN;
  }
  shutdownSubscribers();
  server_->stop();
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

    if (getMountPoints() != getAllMountPoints()) {
      return makeFuture<TakeoverData>(std::runtime_error(
          "can only perform graceful restart when all mount points are initialized"));
      // TODO(xavierd): There is still a potential race after this check if a
      // mount is initiated at this point. Injecting a block below and starting
      // a mount would manifest it. In practice, this should be fairly rare.
      // Moving this further (in stopMountsForTakeover for instance) to avoid
      // this race requires EdenFS to being able to gracefully handle failures
      // and recover in these cases by restarting several components after they
      // have been already shutdown.
    }

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
      .via(serverState_->getThreadPool().get())
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
          XLOG(ERR) << "Failed to compact local store with error: " << e.what()
                    << ". Continuing takeover server shutdown anyway.";
        }

        shutdownSubscribers();

        // Stop the thrift server. In the future, we'd like to
        // keep listening and instead start internally queueing thrift calls
        // to pass with the takeover data below, while waiting here for
        // currently processing thrift calls to finish.
        server_->stop();
      })
      .via(getMainEventBase()) // NFS mounts need to be shut down here
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
                        [](folly::File&& file) {
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
                XLOG(DBG7) << "Got mountd Socket for takeover "
                           << mountdSocket.value().fd();
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
  XLOG(DBG1) << "cancel all subscribers prior to stopping thrift";
  auto mountPoints = mountPoints_->wlock();
  for (auto& entry : *mountPoints) {
    auto& info = entry.second;
    info.edenMount->getJournal().cancelAllSubscribers();
  }
}

void EdenServer::flushStatsNow() {
  serverState_->getStats().flush();
}

void EdenServer::reportMemoryStats() {
  constexpr folly::StringPiece kRssBytes{"memory_vm_rss_bytes"};

  auto memoryStats = facebook::eden::proc_util::readMemoryStats();
  if (memoryStats) {
    // TODO: Stop using the legacy addStatValue() call that checks to see
    // if it needs to re-export counters each time it is used.
    //
    // It's not really even clear to me that it's worth exporting this a
    // timeseries vs a simple counter.  We mainly only care about the
    // last 60-second timeseries level.  Since we only update this once
    // every 30 seconds we are basically just reporting an average of the
    // last 2 data points.
    fb303::ServiceData::get()->addStatValue(
        kRssBytes, memoryStats->resident, fb303::AVG);
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
    for (auto& entry : *lockedStores) {
      backingStores.emplace_back(entry.second);
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

void EdenServer::workingCopyGC() {
  auto config = serverState_->getReloadableConfig()->getEdenConfig();
  auto cutoffConfig =
      std::chrono::duration_cast<std::chrono::system_clock::duration>(
          config->gcCutoff.getValue());

  auto cutoff = std::chrono::system_clock::time_point::max();
  if (cutoffConfig != std::chrono::system_clock::duration::zero()) {
    cutoff = std::chrono::system_clock::now() - cutoffConfig;
  }

  const auto mountPoints = getMountPoints();
  for (const auto& mount : mountPoints) {
    auto rootInode = mount->getRootInode();

    auto lease = mount->tryStartWorkingCopyGC(rootInode);
    if (!lease) {
      XLOG(DBG6) << "Not running GC for: " << mount->getPath()
                 << ", another GC is already in progress";
      continue;
    }

    // Avoid blocking the Thrift EventBase by invalidating on another executor.
    folly::via(getServerState()->getThreadPool().get(), [rootInode, cutoff] {
      static auto context = ObjectFetchContext::getNullContextWithCauseDetail(
          "EdenServer::garbageCollect");
      return rootInode->invalidateChildrenNotMaterialized(cutoff, context)
          .semi();
    }).ensure([rootInode, lease = std::move(lease)] {
      rootInode->unloadChildrenUnreferencedByFs();
    });
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
  XLOG(ERR) << "Stopping EdenFS: on-disk lock file is no longer valid";

  // Attempt an orderly shutdown for now.  Since our state directory may have
  // been deleted we might not really be able to shut down normally, but for now
  // we'll try.  We potentially could try more aggressive measures (exit() or
  // _exit()) if we find that trying to stop normally here ever causes problems.
  stop();
}

} // namespace facebook::eden
