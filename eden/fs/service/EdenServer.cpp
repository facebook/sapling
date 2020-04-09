/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/EdenServer.h"

#include <cpptoml.h> // @manual=fbsource//third-party/cpptoml:cpptoml
#include <algorithm>
#include <atomic>
#include <fstream>
#include <functional>
#include <memory>
#include <numeric>
#include <sstream>

#include <fb303/ServiceData.h>
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>
#include <gflags/gflags.h>
#include <signal.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <thrift/lib/cpp2/transport/rsocket/server/RSRoutingHandler.h>

#include "eden/fs/config/CheckoutConfig.h"
#include "eden/fs/service/EdenCPUThreadPool.h"
#include "eden/fs/service/EdenError.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/store/BlobCache.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/SqliteLocalStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/store/hg/HgQueuedBackingStore.h"
#include "eden/fs/telemetry/EdenStats.h"
#include "eden/fs/telemetry/RequestMetricsScope.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/telemetry/StructuredLoggerFactory.h"
#include "eden/fs/utils/Clock.h"
#include "eden/fs/utils/ProcUtil.h"

#ifdef _WIN32
#include "eden/fs/inodes/win/EdenMount.h" // @manual
#include "eden/fs/win/mount/PrjfsChannel.h" // @manual
#include "eden/fs/win/service/StartupLogger.h" // @manual
#include "eden/fs/win/utils/FileUtils.h" // @manual
#include "eden/fs/win/utils/Stub.h" // @manual
#else
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/Overlay.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverServer.h"
#include "eden/fs/utils/ProcessNameCache.h"
#endif // _WIN32

#ifdef EDEN_HAVE_GIT
#include "eden/fs/store/git/GitBackingStore.h" // @manual
#endif

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

#ifndef _WIN32
#define DEFAULT_STORAGE_ENGINE "rocksdb"
#define SUPPORTED_STORAGE_ENGINES "rocksdb|sqlite|memory"
#else
#define DEFAULT_STORAGE_ENGINE "sqlite"
#define SUPPORTED_STORAGE_ENGINES "sqlite|memory"
#endif

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
DEFINE_int32(
    thrift_num_workers,
    std::thread::hardware_concurrency(),
    "The number of thrift worker threads");
DEFINE_int32(
    thrift_max_requests,
    apache::thrift::concurrency::ThreadManager::DEFAULT_MAX_QUEUE_SIZE,
    "Maximum number of active thrift requests");
DEFINE_bool(thrift_enable_codel, false, "Enable Codel queuing timeout");
DEFINE_int32(thrift_min_compress_bytes, 0, "Minimum response compression size");
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

constexpr StringPiece kRocksDBPath{"storage/rocks-db"};
constexpr StringPiece kSqlitePath{"storage/sqlite.db"};
constexpr StringPiece kConfig{"config.toml"};
static const std::string kHgStorePrefix{"store.hg"};

std::optional<std::string> getUnixDomainSocketPath(
    const folly::SocketAddress& address) {
  return AF_UNIX == address.getFamily() ? std::make_optional(address.getPath())
                                        : std::nullopt;
}

std::string getCounterNameForImportMetric(
    HgQueuedBackingStore::HgImportStage stage,
    RequestMetricsScope::RequestMetric metric,
    std::optional<HgBackingStore::HgImportObject> object = std::nullopt) {
  if (object.has_value()) {
    // base prefix . stage . object . metric
    return folly::join(
        ".",
        {kHgStorePrefix,
         HgQueuedBackingStore::stringOfHgImportStage(stage),
         HgBackingStore::stringOfHgImportObject(object.value()),
         RequestMetricsScope::stringOfRequestMetric(metric)});
  }
  // base prefix . stage . metric
  return folly::join(
      ".",
      {kHgStorePrefix,
       HgQueuedBackingStore::stringOfHgImportStage(stage),
       RequestMetricsScope::stringOfRequestMetric(metric)});
}

} // namespace

namespace facebook {
namespace eden {

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
    std::string version)
    : originalCommandLine_{std::move(originalCommandLine)},
      edenDir_{edenConfig->edenDir.getValue()},
      blobCache_{BlobCache::create(
          FLAGS_maximumBlobCacheSize,
          FLAGS_minimumBlobCacheEntryCount)},
      serverState_{make_shared<ServerState>(
          std::move(userInfo),
          std::move(privHelper),
          std::make_shared<EdenCPUThreadPool>(),
          std::make_shared<UnixClock>(),
          std::make_shared<ProcessNameCache>(),
          makeDefaultStructuredLogger(*edenConfig, std::move(sessionInfo)),
          edenConfig,
          FLAGS_enable_fault_injection)},
      version_{std::move(version)} {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->registerCallback(kBlobCacheMemory, [this] {
    return this->getBlobCache()->getStats().totalSizeInBytes;
  });

  for (auto stage : HgQueuedBackingStore::hgImportStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : HgBackingStore::hgImportObjects) {
        std::string counterName =
            getCounterNameForImportMetric(stage, metric, object);
        counters->registerCallback(counterName, [this, stage, object, metric] {
          auto individual_counters = this->collectHgQueuedBackingStoreCounters(
              [stage, object, metric](const HgQueuedBackingStore& store) {
                return store.getImportMetric(stage, object, metric);
              });
          return this->aggregateHgQueuedBackingStoreCounters(
              metric, individual_counters);
        });
      }
      std::string summaryCounterName =
          getCounterNameForImportMetric(stage, metric);
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
        return this->aggregateHgQueuedBackingStoreCounters(
            metric, individual_counters);
      });
    }
  }
}

EdenServer::~EdenServer() {
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(kBlobCacheMemory);

  for (auto stage : HgQueuedBackingStore::hgImportStages) {
    for (auto metric : RequestMetricsScope::requestMetrics) {
      for (auto object : HgBackingStore::hgImportObjects) {
        std::string counterName =
            getCounterNameForImportMetric(stage, metric, object);
        counters->unregisterCallback(counterName);
      }
      std::string summaryCounterName =
          getCounterNameForImportMetric(stage, metric);
      counters->unregisterCallback(summaryCounterName);
    }
  }
}

Future<Unit> EdenServer::unmountAll() {
#ifndef _WIN32
  std::vector<Future<Unit>> futures;
  {
    const auto mountPoints = mountPoints_.wlock();
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
          result.throwIfFailed();
        }
      });
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

#ifndef _WIN32
Future<TakeoverData> EdenServer::stopMountsForTakeover() {
  std::vector<Future<optional<TakeoverData::MountInfo>>> futures;
  {
    const auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      auto& info = entry.second;

      try {
        info.takeoverPromise.emplace();
        auto future = info.takeoverPromise->getFuture();
        info.edenMount->getFuseChannel()->takeoverStop();
        futures.emplace_back(std::move(future).thenValue(
            [self = this,
             edenMount = info.edenMount](TakeoverData::MountInfo takeover)
                -> Future<optional<TakeoverData::MountInfo>> {
              if (!takeover.fuseFD) {
                return std::nullopt;
              }
              return self->serverState_->getPrivHelper()
                  ->fuseTakeoverShutdown(edenMount->getPath().stringPiece())
                  .thenValue([takeover = std::move(takeover)](auto&&) mutable {
                    return std::move(takeover);
                  });
            }));
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Error while stopping \"" << mountPath
                  << "\" for takeover: " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<optional<TakeoverData::MountInfo>>(
            folly::exception_wrapper(std::current_exception(), ex)));
      }
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).toUnsafeFuture().thenValue(
      [](std::vector<folly::Try<optional<TakeoverData::MountInfo>>> results) {
        TakeoverData data;
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
  // Flush stats must run once every second for accurate aggregation of
  // the time series & histogram buckets
  flushStatsTask_.updateInterval(1s, /*splay=*/false);

#ifndef _WIN32
  // Report memory usage stats once every 30 seconds
  memoryStatsTask_.updateInterval(30s);
#endif
  auto config = serverState_->getReloadableConfig().getEdenConfig();
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

  localStoreTask_.updateInterval(
      std::chrono::duration_cast<std::chrono::milliseconds>(
          config.localStoreManagementInterval.getValue()));
}

#ifndef _WIN32
void EdenServer::unloadInodes() {
  struct Root {
    std::string mountName;
    TreeInodePtr rootInode;
  };
  std::vector<Root> roots;
  {
    const auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      roots.emplace_back(Root{std::string{entry.first},
                              entry.second.edenMount->getRootInode()});
    }
  }

  if (!roots.empty()) {
    auto serviceData = fb303::ServiceData::get();

    uint64_t totalUnloaded = serviceData->getCounter(kPeriodicUnloadCounterKey);
    auto cutoff = std::chrono::system_clock::now() -
        std::chrono::minutes(FLAGS_unload_age_minutes);
    auto cutoff_ts = folly::to<timespec>(cutoff);
    for (auto& [name, rootInode] : roots) {
      auto unloaded = rootInode->unloadChildrenLastAccessedBefore(cutoff_ts);
      if (unloaded) {
        XLOG(INFO) << "Unloaded " << unloaded
                   << " inodes in background from mount " << name;
      }
      totalUnloaded += unloaded;
    }
    serviceData->setCounter(kPeriodicUnloadCounterKey, totalUnloaded);
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
          // running state. Additionally, set the takeoverShutdown state to
          // false in order to allow for future graceful restart requests.
          [this] {
            auto state = runningState_.wlock();
            state->takeoverShutdown = false;
            state->takeoverPromise = folly::Promise<TakeoverData>();
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

Future<Unit> EdenServer::prepare(
    std::shared_ptr<StartupLogger> logger,
    bool waitForMountCompletion) {
  return prepareImpl(std::move(logger), waitForMountCompletion)
      .ensure(
          // Mark the server state as RUNNING once we finish setting up the
          // mount points. Even if an error occurs we still transition to the
          // running state. The prepare() code will log an error with more
          // details if we do fail to set up some of the mount points.
          [this] { runningState_.wlock()->state = RunState::RUNNING; });
}

Future<Unit> EdenServer::prepareImpl(
    std::shared_ptr<StartupLogger> logger,
    bool waitForMountCompletion) {
  bool doingTakeover = false;
  if (!edenDir_.acquireLock()) {
    // Another edenfs process is already running.
    //
    // If --takeover was specified, fall through and attempt to gracefully
    // takeover mount points from the existing daemon.
    //
    // If --takeover was not specified, fail now.
    if (!FLAGS_takeover) {
      throw std::runtime_error(folly::to<string>(
          "another instance of Eden appears to be running for ",
          edenDir_.getPath()));
    }
    doingTakeover = true;
  }
  // Store a pointer to the EventBase that will be used to drive
  // the main thread.  The runServer() code will end up driving this EventBase.
  mainEventBase_ = folly::EventBaseManager::get()->getEventBase();
  auto thriftRunningFuture = createThriftServer();
#ifndef _WIN32
  // Start the PrivHelper client, using our main event base to drive its I/O
  serverState_->getPrivHelper()->attachEventBase(mainEventBase_);
#endif

  // Set the ServiceData counter for tracking number of inodes unloaded by
  // periodic job for unloading inodes to zero on EdenServer start.
  fb303::ServiceData::get()->setCounter(kPeriodicUnloadCounterKey, 0);

  startPeriodicTasks();

#ifndef _WIN32
  // If we are gracefully taking over from an existing edenfs process,
  // receive its lock, thrift socket, and mount points now.
  // This will shut down the old process.
  const auto takeoverPath = edenDir_.getTakeoverSocketPath();
#endif
  TakeoverData takeoverData;
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

  auto config = parseConfig();
  openStorageEngine(config, logger);

#ifndef _WIN32
  // Start listening for graceful takeover requests
  takeoverServer_.reset(new TakeoverServer(
      getMainEventBase(),
      takeoverPath,
      this,
      &serverState_->getFaultInjector()));
  takeoverServer_->start();
#endif // !_WIN32

  std::vector<Future<Unit>> mountFutures;
  if (doingTakeover) {
#ifndef _WIN32
    mountFutures =
        prepareMountsTakeover(logger, std::move(takeoverData.mountPoints));
#else
    NOT_IMPLEMENTED();
#endif // !_WIN32
  } else {
    mountFutures = prepareMounts(logger);
  }

  if (waitForMountCompletion) {
    // Return a future that will complete only when all mount points have
    // started and the thrift server is also running.
    mountFutures.emplace_back(std::move(thriftRunningFuture));
    return folly::collectAllUnsafe(mountFutures).unit();
  } else {
    // Don't wait for the mount futures.
    // Only return the thrift future.
    return thriftRunningFuture;
  }
}

std::shared_ptr<cpptoml::table> EdenServer::parseConfig() {
  auto configPath = edenDir_.getPath() + RelativePathPiece{kConfig};
  std::ifstream inputFile(configPath.c_str());
  if (!inputFile.is_open()) {
    if (errno != ENOENT) {
      folly::throwSystemErrorExplicit(
          errno, "unable to open EdenFS config ", configPath);
    }
    // config file does not yet exist, create file
    auto configRoot = getDefaultConfig();
    std::stringstream stream;
    stream << (*configRoot);

#ifdef _WIN32
    writeFileAtomic(configPath.c_str(), stream.str());
#else
    folly::writeFileAtomic(string(configPath.c_str()), stream.str());
#endif

    return configRoot;
  }
  return cpptoml::parser(inputFile).parse();
}

std::shared_ptr<cpptoml::table> EdenServer::getDefaultConfig() {
  auto configTable = cpptoml::make_table();
  auto storageEngine = FLAGS_local_storage_engine_unsafe.empty()
      ? DEFAULT_STORAGE_ENGINE
      : FLAGS_local_storage_engine_unsafe;
  std::shared_ptr<cpptoml::table> rootTable = cpptoml::make_table();
  configTable->insert("engine", storageEngine);
  rootTable->insert("local-store", configTable);
  return rootTable;
}

void EdenServer::openStorageEngine(
    std::shared_ptr<cpptoml::table> config,
    std::shared_ptr<StartupLogger> logger) {
  std::string storageEngine =
      config->get_qualified_as<std::string>("local-store.engine").value_or("");
  if (!FLAGS_local_storage_engine_unsafe.empty() &&
      FLAGS_local_storage_engine_unsafe != storageEngine) {
    throw std::runtime_error(folly::to<string>(
        "--local_storage_engine_unsafe flag ",
        FLAGS_local_storage_engine_unsafe,
        "does not match last recorded flag ",
        storageEngine));
  }

  if (storageEngine == "memory") {
    logger->log("Creating new memory store.");
    localStore_ = make_shared<MemoryLocalStore>();
  } else if (storageEngine == "sqlite") {
    const auto path = edenDir_.getPath() + RelativePathPiece{kSqlitePath};
    const auto parentDir = path.dirname();
    ensureDirectoryExists(parentDir);
    logger->log("Opening local SQLite store ", path, "...");
    folly::stop_watch<std::chrono::milliseconds> watch;
    localStore_ = make_shared<SqliteLocalStore>(path);
    logger->log(
        "Opened SQLite store in ",
        watch.elapsed().count() / 1000.0,
        " seconds.");
#ifndef _WIN32
  } else if (storageEngine == "rocksdb") {
    logger->log("Opening local RocksDB store...");
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
    logger->log(
        "Opened RocksDB store in ",
        watch.elapsed().count() / 1000.0,
        " seconds.");

#endif // !_WIN32
  } else {
    throw std::runtime_error(folly::to<string>(
        "invalid storage engine: ", FLAGS_local_storage_engine_unsafe));
  }
}

std::vector<Future<Unit>> EdenServer::prepareMountsTakeover(
    shared_ptr<StartupLogger> logger,
    std::vector<TakeoverData::MountInfo>&& takeoverMounts) {
  // Trigger remounting of existing mount points
  // If doingTakeover is true, use the mounts received in TakeoverData
  std::vector<Future<Unit>> mountFutures;
#ifndef _WIN32
  for (auto& info : takeoverMounts) {
    const auto stateDirectory = info.stateDirectory;
    auto mountFuture =
        makeFutureWith([&] {
          auto initialConfig = CheckoutConfig::loadFromClientDirectory(
              AbsolutePathPiece{info.mountPath},
              AbsolutePathPiece{info.stateDirectory});
          return mount(std::move(initialConfig), false, std::move(info));
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
#else
  NOT_IMPLEMENTED();
#endif
  return mountFutures;
}

std::vector<Future<Unit>> EdenServer::prepareMounts(
    shared_ptr<StartupLogger> logger) {
  std::vector<Future<Unit>> mountFutures;
  folly::dynamic dirs = folly::dynamic::object();
  try {
    dirs = CheckoutConfig::loadClientDirectoryMap(edenDir_.getPath());
  } catch (const std::exception& ex) {
    incrementStartupMountFailures();
    logger->warn(
        "Could not parse config.json file: ",
        ex.what(),
        "\nSkipping remount step.");
    mountFutures.emplace_back(
        folly::exception_wrapper(std::current_exception(), ex));
    return mountFutures;
  }

  if (dirs.empty()) {
    logger->log("No mount points currently configured.");
    return mountFutures;
  }
  logger->log("Remounting ", dirs.size(), " mount points...");

  for (const auto& client : dirs.items()) {
    auto mountFuture =
        makeFutureWith([&] {
          MountInfo mountInfo;
          mountInfo.mountPoint = client.first.c_str();
          auto edenClientPath =
              edenDir_.getCheckoutStateDir(client.second.asString());
          mountInfo.edenClientPath = edenClientPath.stringPiece().str();
          auto initialConfig = CheckoutConfig::loadFromClientDirectory(
              AbsolutePathPiece{mountInfo.mountPoint},
              AbsolutePathPiece{mountInfo.edenClientPath});
          return mount(std::move(initialConfig), false);
        })
            .thenTry([logger, mountPath = client.first.asString()](
                         folly::Try<std::shared_ptr<EdenMount>>&& result) {
              if (result.hasValue()) {
                logger->log("Successfully remounted ", mountPath);
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

  folly::File thriftSocket;
  {
    auto state = runningState_.wlock();
    takeover = state->takeoverShutdown;
    if (takeover) {
      thriftSocket = std::move(state->takeoverThriftSocket);
    }
    state->state = RunState::SHUTTING_DOWN;
  }
  auto shutdownFuture = takeover
      ? performTakeoverShutdown(std::move(thriftSocket))
      : performNormalShutdown().thenValue([](auto&&) { return std::nullopt; });

  // Drive the main event base until shutdownFuture completes
  CHECK_EQ(mainEventBase_, folly::EventBaseManager::get()->getEventBase());
  while (!shutdownFuture.isReady()) {
    mainEventBase_->loopOnce();
  }
  auto&& shutdownResult = shutdownFuture.getTry();
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
  shutdownResult.throwIfFailed();

  return true;
}

Future<optional<TakeoverData>> EdenServer::performTakeoverShutdown(
    folly::File thriftSocket) {
#ifndef _WIN32
  // stop processing new FUSE requests for the mounts,
  return stopMountsForTakeover().thenValue(
      [this,
       socket = std::move(thriftSocket)](TakeoverData&& takeover) mutable {
        takeover.lockFile = edenDir_.extractLock();
        auto future = takeover.takeoverComplete.getFuture();
        takeover.thriftSocket = std::move(socket);

        runningState_.wlock()->takeoverPromise.setValue(std::move(takeover));
        return future;
      });
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

Future<Unit> EdenServer::performNormalShutdown() {
#ifndef _WIN32
  takeoverServer_.reset();

  // Clean up all the server mount points before shutting down the privhelper.
  // Return an uninitalized optional here to avoid an attempted recovery
  return unmountAll();
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
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
  auto mountPath = edenMount->getPath().stringPiece();
  {
    const auto mountPoints = mountPoints_.wlock();
    const auto ret = mountPoints->emplace(mountPath, EdenMountInfo(edenMount));
    if (!ret.second) {
      throw newEdenError(
          EEXIST,
          EdenErrorType::POSIX_ERROR,
          folly::to<string>(
              "mount point \"", mountPath, "\" is already mounted"));
    }
  }
}

void EdenServer::registerStats(std::shared_ptr<EdenMount> edenMount) {
#ifndef _WIN32
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
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServer::unregisterStats(EdenMount* edenMount) {
#ifndef _WIN32
  auto counters = fb303::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::INODEMAP_LOADED));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::INODEMAP_UNLOADED));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MEMORY));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_ENTRIES));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_DURATION));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::JOURNAL_MAX_FILES_ACCUMULATED));
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

#ifndef _WIN32
folly::Future<folly::Unit> EdenServer::performFreshFuseStart(
    std::shared_ptr<EdenMount> edenMount,
    bool readOnly) {
  // Start up the fuse workers.
  return edenMount->startFuse(readOnly);
}
#endif // !_WIN32

#ifndef _WIN32
Future<Unit> EdenServer::performTakeoverFuseStart(
    std::shared_ptr<EdenMount> edenMount,
    TakeoverData::MountInfo&& info) {
  std::vector<std::string> bindMounts;
  for (const auto& bindMount : info.bindMounts) {
    bindMounts.emplace_back(bindMount.value());
  }
  auto future = serverState_->getPrivHelper()->fuseTakeoverStartup(
      info.mountPath.stringPiece(), bindMounts);
  return std::move(future).thenValue([this,
                                      edenMount = std::move(edenMount),
                                      info = std::move(info)](auto&&) mutable {
    return completeTakeoverFuseStart(std::move(edenMount), std::move(info));
  });
}

Future<Unit> EdenServer::completeTakeoverFuseStart(
    std::shared_ptr<EdenMount> edenMount,
    TakeoverData::MountInfo&& info) {
  FuseChannelData channelData;
  channelData.fd = std::move(info.fuseFD);
  channelData.connInfo = info.connInfo;

  // Start up the fuse workers.
  return folly::makeFutureWith(
      [&] { edenMount->takeoverFuse(std::move(channelData)); });
}
#endif // !_WIN32

folly::Future<std::shared_ptr<EdenMount>> EdenServer::mount(
    std::unique_ptr<CheckoutConfig> initialConfig,
    bool readOnly,
    optional<TakeoverData::MountInfo>&& optionalTakeover) {
  folly::stop_watch<> mountStopWatch;

  auto backingStore = getBackingStore(
      initialConfig->getRepoType(), initialConfig->getRepoSource());
  auto objectStore = ObjectStore::create(
      getLocalStore(),
      backingStore,
      getSharedStats(),
      serverState_->getThreadPool().get());
  auto journal = std::make_unique<Journal>(getSharedStats());

#if _WIN32
  // Create the EdenMount object and insert the mount into the mountPoints_
  // map.
  auto edenMount = EdenMount::create(
      std::move(initialConfig),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal));
  edenMount->initialize(std::make_unique<PrjfsChannel>(edenMount.get()));
  addToMountPoints(edenMount);
  edenMount->start();
  (void)mountStopWatch;
  return makeFuture<std::shared_ptr<EdenMount>>(std::move(edenMount));

#else
  // Create the EdenMount object and insert the mount into the mountPoints_ map.
  auto edenMount = EdenMount::create(
      std::move(initialConfig),
      std::move(objectStore),
      blobCache_,
      serverState_,
      std::move(journal));
  addToMountPoints(edenMount);
  registerStats(edenMount);

  // Now actually begin starting the mount point
  const bool doTakeover = optionalTakeover.has_value();
  auto initFuture = edenMount->initialize(
      optionalTakeover ? std::make_optional(optionalTakeover->inodeMap)
                       : std::nullopt);
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
        return (optionalTakeover ? performTakeoverFuseStart(
                                       edenMount, std::move(*optionalTakeover))
                                 : performFreshFuseStart(edenMount, readOnly))
            .thenTry([edenMount, doTakeover, this](
                         folly::Try<Unit>&& result) mutable {
              // Call mountFinished() if an error occurred during FUSE
              // initialization.
              if (result.hasException()) {
                mountFinished(edenMount.get(), std::nullopt);
                return makeFuture<shared_ptr<EdenMount>>(
                    std::move(result).exception());
              }

              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              auto finishFuture = edenMount->getFuseCompletionFuture().thenTry(
                  [this,
                   edenMount](folly::Try<TakeoverData::MountInfo>&& takeover) {
                    std::optional<TakeoverData::MountInfo> optTakeover;
                    if (takeover.hasValue()) {
                      optTakeover = std::move(takeover.value());
                    }
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
              event.repo_type = edenMount->getConfig()->getRepoType();
              event.repo_source =
                  basename(edenMount->getConfig()->getRepoSource()).str();
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
#endif
}

Future<Unit> EdenServer::unmount(StringPiece mountPath) {
#ifndef _WIN32
  return makeFutureWith([&] {
           auto future = Future<Unit>::makeEmpty();
           auto mount = std::shared_ptr<EdenMount>{};
           {
             const auto mountPoints = mountPoints_.wlock();
             const auto it = mountPoints->find(mountPath);
             if (it == mountPoints->end()) {
               return makeFuture<Unit>(
                   std::out_of_range("no such mount point " + mountPath.str()));
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
      .thenError([path = mountPath.str()](folly::exception_wrapper&& ew) {
        XLOG(ERR) << "Failed to perform unmount for \"" << path
                  << "\": " << folly::exceptionStr(ew);
        return makeFuture<Unit>(std::move(ew));
      });
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServer::mountFinished(
    EdenMount* edenMount,
    std::optional<TakeoverData::MountInfo> takeover) {
#ifndef _WIN32
  const auto mountPath = edenMount->getPath().stringPiece();
  XLOG(INFO) << "mount point \"" << mountPath << "\" stopped";
  unregisterStats(edenMount);

  // Erase the EdenMount from our mountPoints_ map
  folly::SharedPromise<Unit> unmountPromise;
  std::optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;
  {
    const auto mountPoints = mountPoints_.wlock();
    const auto it = mountPoints->find(mountPath);
    CHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    takeoverPromise = std::move(it->second.takeoverPromise);
    mountPoints->erase(it);
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
              result.throwIfFailed();
              return Unit{};
            }));
      });
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

EdenServer::MountList EdenServer::getMountPoints() const {
  MountList results;
  {
    const auto mountPoints = mountPoints_.rlock();
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
    const auto mountPoints = mountPoints_.rlock();
    for (const auto& entry : *mountPoints) {
      results.emplace_back(entry.second.edenMount);
    }
  }
  return results;
}

shared_ptr<EdenMount> EdenServer::getMount(StringPiece mountPath) const {
  const auto mount = getMountUnsafe(mountPath);
  if (!mount->isSafeForInodeAccess()) {
    throw newEdenError(
        EBUSY,
        EdenErrorType::POSIX_ERROR,
        folly::to<string>(
            "mount point \"", mountPath, "\" is still initializing"));
  }
  return mount;
}

shared_ptr<EdenMount> EdenServer::getMountUnsafe(StringPiece mountPath) const {
  const auto mountPoints = mountPoints_.rlock();
  const auto it = mountPoints->find(mountPath);
  if (it == mountPoints->end()) {
    throw newEdenError(
        ENOENT,
        EdenErrorType::POSIX_ERROR,
        folly::to<string>(
            "mount point \"",
            mountPath,
            "\" is not known to this eden instance"));
  }
  return it->second.edenMount;
}

shared_ptr<BackingStore> EdenServer::getBackingStore(
    StringPiece type,
    StringPiece name) {
  BackingStoreKey key{type.str(), name.str()};
  auto lockedStores = backingStores_.wlock();
  const auto it = lockedStores->find(key);
  if (it != lockedStores->end()) {
    return it->second;
  }

  const auto store = createBackingStore(type, name);
  lockedStores->emplace(key, store);
  return store;
}

std::unordered_set<shared_ptr<HgQueuedBackingStore>>
EdenServer::getHgQueuedBackingStores() {
  std::unordered_set<std::shared_ptr<HgQueuedBackingStore>> hgBackingStores{};
  {
    auto lockedStores = this->backingStores_.rlock();
    for (auto entry : *lockedStores) {
      if (auto store =
              std::dynamic_pointer_cast<HgQueuedBackingStore>(entry.second)) {
        hgBackingStores.emplace(std::move(store));
      }
    }
  }
  return hgBackingStores;
}

size_t EdenServer::aggregateHgQueuedBackingStoreCounters(
    RequestMetricsScope::RequestMetric metric,
    std::vector<size_t>& counters) {
  switch (metric) {
    case RequestMetricsScope::RequestMetric::COUNT:
      return std::accumulate(counters.begin(), counters.end(), 0);
    case RequestMetricsScope::RequestMetric::MAX_DURATION_US:
      auto max = std::max_element(counters.begin(), counters.end());
      return max == counters.end() ? 0 : *max;
  }
}

std::vector<size_t> EdenServer::collectHgQueuedBackingStoreCounters(
    std::function<size_t(const HgQueuedBackingStore&)> getCounterFromStore) {
  std::vector<size_t> counters;
  for (const auto& store : this->getHgQueuedBackingStores()) {
    counters.emplace_back(getCounterFromStore(*store));
  }
  return counters;
}

shared_ptr<BackingStore> EdenServer::createBackingStore(
    StringPiece type,
    StringPiece name) {
  if (type == "null") {
    return make_shared<EmptyBackingStore>();
  } else if (type == "hg") {
    const auto repoPath = realpath(name);
    auto store = std::make_unique<HgBackingStore>(
        repoPath,
        localStore_.get(),
        serverState_->getThreadPool().get(),
        shared_ptr<ReloadableConfig>(
            serverState_, &serverState_->getReloadableConfig()),
        getSharedStats());
    return make_shared<HgQueuedBackingStore>(std::move(store));
  } else if (type == "git") {
#ifdef EDEN_HAVE_GIT
    const auto repoPath = realpath(name);
    return make_shared<GitBackingStore>(repoPath, localStore_.get());
#else // EDEN_HAVE_GIT
    throw std::domain_error(
        "support for Git was not enabled in this EdenFS build");
#endif // EDEN_HAVE_GIT
  }
  throw std::domain_error(
      folly::to<string>("unsupported backing store type: ", type));
}

Future<Unit> EdenServer::createThriftServer() {
  server_ = make_shared<ThriftServer>();
  server_->setMaxRequests(FLAGS_thrift_max_requests);
  server_->setNumIOWorkerThreads(FLAGS_thrift_num_workers);
  server_->setEnableCodel(FLAGS_thrift_enable_codel);
  server_->setMinCompressBytes(FLAGS_thrift_min_compress_bytes);
  server_->addRoutingHandler(
      std::make_unique<apache::thrift::RSRoutingHandler>());

  handler_ = make_shared<EdenServiceHandler>(originalCommandLine_, this);
  server_->setInterface(handler_);

  // Get the path to the thrift socket.
  auto thriftSocketPath = edenDir_.getThriftSocketPath();
  folly::SocketAddress thriftAddress;
  thriftAddress.setFromPath(thriftSocketPath.stringPiece());
  server_->setAddress(thriftAddress);
  serverState_->setSocketPath(thriftSocketPath);

  serverEventHandler_ = make_shared<ThriftServerEventHandler>(this);
  server_->setServerEventHandler(serverEventHandler_);
  return serverEventHandler_->getThriftRunningFuture();
}

void EdenServer::prepareThriftAddress() {
  // If we are serving on a local Unix socket, remove any old socket file
  // that may be left over from a previous instance.
  // We have already acquired the mount point lock at this time, so we know
  // that any existing socket is unused and safe to remove.
  const auto& path = getUnixDomainSocketPath(server_->getAddress());
  if (!path) {
    return;
  }
  const int rc = unlink(path->c_str());
  if (rc != 0 && errno != ENOENT) {
    // This might happen if we don't have permission to remove the file.
    folly::throwSystemError("unable to remove old Eden thrift socket ", *path);
  }
}

void EdenServer::stop() {
  shutdownSubscribers();
  server_->stop();
}

folly::Future<TakeoverData> EdenServer::startTakeoverShutdown() {
#ifndef _WIN32
  // Make sure we aren't already shutting down, then update our state
  // to indicate that we should perform mount point takeover shutdown
  // once runServer() returns.
  auto result = Future<TakeoverData>::makeEmpty();
  {
    auto state = runningState_.wlock();
    if (state->state != RunState::RUNNING) {
      // We are either still in the process of starting,
      // or already shutting down.
      return makeFuture<TakeoverData>(std::runtime_error(folly::to<string>(
          "can only perform graceful restart when running normally; "
          "current state is ",
          static_cast<int>(state->state))));
    }
    if (state->takeoverShutdown) {
      // This can happen if startTakeoverShutdown() is called twice
      // before runServer() exits.
      return makeFuture<TakeoverData>(std::runtime_error(
          "another takeover shutdown has already been started"));
    }

    state->takeoverShutdown = true;

    // Make a copy of the thrift server socket so we can transfer it to the
    // new edenfs process.  Our local thrift will close its own socket when
    // we stop the server.  The easiest way to avoid completely closing the
    // server socket for now is simply by duplicating the socket to a new
    // fd. We will transfer this duplicated FD to the new edenfs process.
    const int takeoverThriftSocket = dup(server_->getListenSocket());
    folly::checkUnixError(
        takeoverThriftSocket,
        "error duplicating thrift server socket during graceful takeover");
    state->takeoverThriftSocket =
        folly::File{takeoverThriftSocket, /* ownsFd */ true};
    result = state->takeoverPromise.getFuture();
  }

  // Compact storage for all key spaces in order to speed up the
  // takeover start of the new process. We could potentially test this more
  // and change it in the future to simply flush instead of compact if this
  // proves to be too expensive.
  localStore_->compactStorage();

  shutdownSubscribers();

  // Stop the thrift server.  We will fulfill takeoverPromise once it
  // stops.
  server_->stop();
  return result;
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
#ifndef _WIN32
  XLOG(DBG1) << "cancel all subscribers prior to stopping thrift";
  auto mountPoints = mountPoints_.wlock();
  for (auto& entry : *mountPoints) {
    auto& info = entry.second;
    info.edenMount->getJournal().cancelAllSubscribers();
  }
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServer::flushStatsNow() {
  serverState_->getStats().aggregate();
}

void EdenServer::reportMemoryStats() {
#ifndef _WIN32
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
#else
  NOT_IMPLEMENTED();
#endif // !_WIN32
}

void EdenServer::manageLocalStore() {
  auto config = serverState_->getReloadableConfig().getEdenConfig(
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

void EdenServer::reloadConfig() {
  // Get the config, forcing a reload now.
  auto config = serverState_->getReloadableConfig().getEdenConfig(
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

} // namespace eden
} // namespace facebook
