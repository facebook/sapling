/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/EdenServer.h"

#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/chrono/Conv.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <signal.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "common/stats/ServiceData.h"
#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/DirHandle.h"
#include "eden/fs/fuse/FileHandle.h"
#include "eden/fs/fuse/FileHandleBase.h"
#include "eden/fs/fuse/FuseChannel.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/EdenDispatcher.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/service/EdenCPUThreadPool.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/store/SqliteLocalStore.h"
#include "eden/fs/store/git/GitBackingStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverServer.h"
#include "eden/fs/utils/Clock.h"

DEFINE_bool(
    debug,
    false,
    "run fuse in debug mode"); // TODO: remove; no longer needed
DEFINE_bool(
    takeover,
    false,
    "If another edenfs process is already running, "
    "attempt to gracefully takeover its mount points.");

DEFINE_string(
    local_storage_engine_unsafe,
    "rocksdb",
    "Select storage engine. rocksdb is the default. "
    "possible choices are (rocksdb|sqlite|memory). "
    "memory is currently very dangerous as you will "
    "lose state across restarts and graceful restarts! "
    "It is unsafe to change this between edenfs invocations!");

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
DEFINE_int64(unload_interval_hours, 0, "Frequency of unloading inodes");
DEFINE_int64(
    start_delay_minutes,
    10,
    "start delay for scheduling unloading inodes job");
DEFINE_int64(
    unload_age_minutes,
    60,
    "Minimum age of the inodes to be unloaded");

using apache::thrift::ThriftServer;
using facebook::eden::FuseChannelData;
using folly::File;
using folly::Future;
using folly::makeFuture;
using folly::Optional;
using folly::StringPiece;
using folly::Unit;
using std::make_shared;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace {
using namespace facebook::eden;

constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kThriftSocketName{"socket"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kRocksDBPath{"storage/rocks-db"};
constexpr StringPiece kSqlitePath{"storage/sqlite.db"};
} // namespace

namespace facebook {
namespace eden {

class EdenServer::ThriftServerEventHandler
    : public apache::thrift::server::TServerEventHandler,
      public folly::AsyncSignalHandler {
 public:
  explicit ThriftServerEventHandler(EdenServer* edenServer)
      : AsyncSignalHandler{nullptr}, edenServer_{edenServer} {}

  void preServe(const folly::SocketAddress* /*address*/) override {
    // preServe() will be called from the thrift server thread once when it is
    // about to start serving.
    //
    // Register for SIGINT and SIGTERM.  We do this in preServe() so we can use
    // the thrift server's EventBase to process the signal callbacks.
    auto eventBase = folly::EventBaseManager::get()->getEventBase();
    attachEventBase(eventBase);
    registerSignalHandler(SIGINT);
    registerSignalHandler(SIGTERM);
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

 private:
  EdenServer* edenServer_{nullptr};
};

EdenServer::EdenServer(
    UserInfo userInfo,
    std::unique_ptr<PrivHelper> privHelper,
    AbsolutePathPiece edenDir,
    AbsolutePathPiece etcEdenDir,
    AbsolutePathPiece configPath)
    : edenDir_(edenDir),
      etcEdenDir_(etcEdenDir),
      configPath_(configPath),
      serverState_{make_shared<ServerState>(
          std::move(userInfo),
          std::move(privHelper),
          std::make_shared<EdenCPUThreadPool>(),
          std::make_shared<UnixClock>())} {}

EdenServer::~EdenServer() {}

Future<Unit> EdenServer::unmountAll() {
  std::vector<Future<Unit>> futures;
  {
    const auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      auto& info = entry.second;

      try {
        serverState_->getPrivHelper()->fuseUnmount(mountPath);
        futures.emplace_back(info.unmountPromise.getFuture());
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Failed to perform unmount for \"" << mountPath
                  << "\": " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<Unit>(
            folly::exception_wrapper(std::current_exception(), ex)));
      }
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAllSemiFuture(futures).toUnsafeFuture().then(
      [](std::vector<folly::Try<Unit>> results) {
        for (const auto& result : results) {
          result.throwIfFailed();
        }
      });
}

Future<TakeoverData> EdenServer::stopMountsForTakeover() {
  std::vector<Future<Optional<TakeoverData::MountInfo>>> futures;
  {
    const auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      auto& info = entry.second;

      try {
        info.takeoverPromise.emplace();
        auto future = info.takeoverPromise->getFuture();
        info.edenMount->getFuseChannel()->takeoverStop();
        futures.emplace_back(future.then(
            [self = this,
             edenMount = info.edenMount](TakeoverData::MountInfo takeover)
                -> Optional<TakeoverData::MountInfo> {
              if (!takeover.fuseFD) {
                return folly::none;
              }
              self->serverState_->getPrivHelper()->fuseTakeoverShutdown(
                  edenMount->getPath().stringPiece());
              return takeover;
            }));
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Error while stopping \"" << mountPath
                  << "\" for takeover: " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<Optional<TakeoverData::MountInfo>>(
            folly::exception_wrapper(std::current_exception(), ex)));
      }
    }
  }
  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAllSemiFuture(futures).toUnsafeFuture().then(
      [](std::vector<folly::Try<Optional<TakeoverData::MountInfo>>> results) {
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
          if (!result.value().hasValue()) {
            XLOG(WARN) << "mount point was unmounted during "
                          "takeover shutdown";
            continue;
          }

          data.mountPoints.emplace_back(std::move(result.value().value()));
        }
        return data;
      });
}

void EdenServer::scheduleFlushStats() {
  mainEventBase_->timer().scheduleTimeoutFn(
      [this] {
        flushStatsNow();
        scheduleFlushStats();
      },
      std::chrono::seconds(1));
}

void EdenServer::unloadInodes() {
  std::vector<TreeInodePtr> roots;
  {
    const auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      roots.emplace_back(entry.second.edenMount->getRootInode());
    }
  }

  if (!roots.empty()) {
    XLOG(INFO) << "UnloadInodeScheduler Unloading Free Inodes";
    auto serviceData = stats::ServiceData::get();

    uint64_t totalUnloaded = serviceData->getCounter(kPeriodicUnloadCounterKey);
    for (auto& rootInode : roots) {
      auto cutoff = std::chrono::system_clock::now() -
          std::chrono::minutes(FLAGS_unload_age_minutes);
      auto cutoff_ts = folly::to<timespec>(cutoff);
      totalUnloaded += rootInode->unloadChildrenLastAccessedBefore(cutoff_ts);
    }
    serviceData->setCounter(kPeriodicUnloadCounterKey, totalUnloaded);
  }

  scheduleInodeUnload(std::chrono::hours(FLAGS_unload_interval_hours));
}

void EdenServer::scheduleInodeUnload(std::chrono::milliseconds timeout) {
  mainEventBase_->timer().scheduleTimeoutFn(
      [this] { unloadInodes(); }, timeout);
}

void EdenServer::prepare() {
  bool doingTakeover = false;
  if (!acquireEdenLock()) {
    // Another edenfs process is already running.
    //
    // If --takeover was specified, fall through and attempt to gracefully
    // takeover mount points from the existing daemon.
    //
    // If --takeover was not specified, fail now.
    if (!FLAGS_takeover) {
      throw std::runtime_error(
          "another instance of Eden appears to be running for " +
          edenDir_.stringPiece().str());
    }
    doingTakeover = true;
  }

  // Store a pointer to the EventBase that will be used to drive
  // the main thread.  The runServer() code will end up driving this EventBase.
  mainEventBase_ = folly::EventBaseManager::get()->getEventBase();
  createThriftServer();

  // Start stats aggregation
  scheduleFlushStats();

  // Set the ServiceData counter for tracking number of inodes unloaded by
  // periodic job for unloading inodes to zero on EdenServer start.
  stats::ServiceData::get()->setCounter(kPeriodicUnloadCounterKey, 0);

  // Schedule a periodic job to unload unused inodes based on the last access
  // time. currently Eden does not have accurate timestamp tracking for inodes,
  // so using unloadChildrenNow just to validate the behaviour. We will have to
  // modify current unloadChildrenNow function to unload inodes based on the
  // last access time.
  if (FLAGS_unload_interval_hours > 0) {
    scheduleInodeUnload(std::chrono::minutes(FLAGS_start_delay_minutes));
  }

  // If we are gracefully taking over from an existing edenfs process,
  // receive its lock, thrift socket, and mount points now.
  // This will shut down the old process.
  const auto takeoverPath = edenDir_ + "takeover"_pc;
  TakeoverData takeoverData;
  if (doingTakeover) {
    takeoverData = takeoverMounts(takeoverPath);

    // Take over the eden lock file and the thrift server socket.
    lockFile_ = std::move(takeoverData.lockFile);
    server_->useExistingSocket(takeoverData.thriftSocket.release());
  } else {
    // Remove any old thrift socket from a previous (now dead) edenfs daemon.
    prepareThriftAddress();
  }

  if (FLAGS_local_storage_engine_unsafe == "memory") {
    XLOG(DBG2) << "Creating new memory store";
    localStore_ = make_shared<MemoryLocalStore>();
  } else if (FLAGS_local_storage_engine_unsafe == "sqlite") {
    const auto path = edenDir_ + RelativePathPiece{kSqlitePath};
    XLOG(DBG2) << "opening local Sqlite store " << path;
    localStore_ = make_shared<SqliteLocalStore>(path);
    XLOG(DBG2) << "done opening local Sqlite store";
  } else if (FLAGS_local_storage_engine_unsafe == "rocksdb") {
    XLOG(DBG2) << "opening local RocksDB store";
    const auto rocksPath = edenDir_ + RelativePathPiece{kRocksDBPath};
    localStore_ = make_shared<RocksDbLocalStore>(rocksPath);
    XLOG(DBG2) << "done opening local RocksDB store";
  } else {
    XLOG(FATAL) << "invalid load_storage_engine flag: "
                << FLAGS_local_storage_engine_unsafe;
  }

  // Start listening for graceful takeover requests
  takeoverServer_.reset(
      new TakeoverServer(getMainEventBase(), takeoverPath, this));
  takeoverServer_->start();

  // Remount existing mount points
  // if doingTakeover is true, use the mounts received in TakeoverData
  if (doingTakeover) {
    for (auto& info : takeoverData.mountPoints) {
      const auto stateDirectory = info.stateDirectory;
      try {
        auto initialConfig = ClientConfig::loadFromClientDirectory(
            AbsolutePathPiece{info.mountPath},
            AbsolutePathPiece{info.stateDirectory});
        mount(std::move(initialConfig), std::move(info)).get();
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Failed to perform takeover for " << stateDirectory << ": "
                  << ex.what();
      }
    }
  } else {
    folly::dynamic dirs = folly::dynamic::object();
    try {
      dirs = ClientConfig::loadClientDirectoryMap(edenDir_);
    } catch (const std::exception& ex) {
      XLOG(ERR) << "Could not parse config.json file: " << ex.what()
                << " Skipping remount step.";
    }
    for (const auto& client : dirs.items()) {
      MountInfo mountInfo;
      mountInfo.mountPoint = client.first.c_str();
      auto edenClientPath = edenDir_ + PathComponent("clients") +
          PathComponent(client.second.c_str());
      mountInfo.edenClientPath = edenClientPath.stringPiece().str();
      try {
        auto initialConfig = ClientConfig::loadFromClientDirectory(
            AbsolutePathPiece{mountInfo.mountPoint},
            AbsolutePathPiece{mountInfo.edenClientPath});
        mount(std::move(initialConfig)).get();
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Failed to perform remount for " << client.first.c_str()
                  << ": " << ex.what();
      }
    }
  }
}

// Defined separately in RunServer.cpp
void runServer(const EdenServer& server);

void EdenServer::run() {
  // Acquire the eden lock, prepare the thrift server, and start our mounts
  prepare();

  // Start listening for graceful takeover requests
  const auto takeoverPath = edenDir_ + PathComponentPiece{kTakeoverSocketName};
  takeoverServer_.reset(
      new TakeoverServer(getMainEventBase(), takeoverPath, this));
  takeoverServer_->start();

  // Run the thrift server
  runningState_.wlock()->state = RunState::RUNNING;
  runServer(*this);

  bool takeover;
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
      : performNormalShutdown();

  // Drive the main event base until shutdownFuture completes
  CHECK_EQ(mainEventBase_, folly::EventBaseManager::get()->getEventBase());
  while (!shutdownFuture.isReady()) {
    mainEventBase_->loopOnce();
  }
  shutdownFuture.get();
}

Future<Unit> EdenServer::performTakeoverShutdown(folly::File thriftSocket) {
  // stop processing new FUSE requests for the mounts,
  return stopMountsForTakeover().then([this, socket = std::move(thriftSocket)](
                                          TakeoverData&& takeover) mutable {
    // Destroy the local store and backing stores.
    // We shouldn't access the local store any more after giving up our
    // lock, and we need to close it to release its lock before the new
    // edenfs process tries to open it.
    backingStores_.wlock()->clear();
    // Explicit close the LocalStore before we reset our pointer, to
    // ensure we release the RocksDB lock.  Since this is managed with a
    // shared_ptr it is somewhat hard to confirm if we really have the
    // last reference to it.
    localStore_->close();
    localStore_.reset();

    // Stop the privhelper process.
    shutdownPrivhelper();

    takeover.lockFile = std::move(lockFile_);
    auto future = takeover.takeoverComplete.getFuture();
    takeover.thriftSocket = std::move(socket);

    takeoverPromise_.setValue(std::move(takeover));
    return future;
  });
}

Future<Unit> EdenServer::performNormalShutdown() {
  takeoverServer_.reset();

  // Clean up all the server mount points before shutting down the privhelper.
  return unmountAll().then([this](folly::Try<Unit>&& result) {
    shutdownPrivhelper();
    result.throwIfFailed();
  });
}

void EdenServer::shutdownPrivhelper() {
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
}

void EdenServer::addToMountPoints(std::shared_ptr<EdenMount> edenMount) {
  auto mountPath = edenMount->getPath().stringPiece();
  {
    const auto mountPoints = mountPoints_.wlock();
    const auto ret = mountPoints->emplace(mountPath, EdenMountInfo(edenMount));
    if (!ret.second) {
      // This mount point already exists.
      throw EdenError(folly::to<string>(
          "mount point \"", mountPath, "\" is already mounted"));
    }
  }
}

void EdenServer::registerStats(std::shared_ptr<EdenMount> edenMount) {
  auto counters = stats::ServiceData::get()->getDynamicCounters();
  // Register callback for getting Loaded inodes in the memory
  // for a mountPoint.
  counters->registerCallback(
      edenMount->getCounterName(CounterName::LOADED),
      [edenMount] { return edenMount->getInodeMap()->getLoadedInodeCount(); });
  // Register callback for getting Unloaded inodes in the
  // memory for a mountpoint
  counters->registerCallback(
      edenMount->getCounterName(CounterName::UNLOADED), [edenMount] {
        return edenMount->getInodeMap()->getUnloadedInodeCount();
      });
}

void EdenServer::unregisterStats(EdenMount* edenMount) {
  auto counters = stats::ServiceData::get()->getDynamicCounters();
  counters->unregisterCallback(edenMount->getCounterName(CounterName::LOADED));
  counters->unregisterCallback(
      edenMount->getCounterName(CounterName::UNLOADED));
}

folly::Future<folly::Unit> EdenServer::performFreshFuseStart(
    std::shared_ptr<EdenMount> edenMount) {
  // Start up the fuse workers.
  return edenMount->startFuse();
}

folly::Future<folly::Unit> EdenServer::performTakeoverFuseStart(
    std::shared_ptr<EdenMount> edenMount,
    TakeoverData::MountInfo&& info) {
  std::vector<std::string> bindMounts;
  for (const auto& bindMount : info.bindMounts) {
    bindMounts.emplace_back(bindMount.value());
  }
  serverState_->getPrivHelper()->fuseTakeoverStartup(
      info.mountPath.stringPiece(), bindMounts);

  // (re)open file handles for each entry in info.fileHandleMap
  std::vector<folly::Future<folly::Unit>> futures;
  auto dispatcher = edenMount->getDispatcher();

  for (const auto& handleEntry : info.fileHandleMap.entries) {
    if (handleEntry.isDir) {
      futures.emplace_back(
          // TODO: we should record the opendir() flags in the
          // SerializedFileHandleMap so that we can restore
          // the correct flags here.
          dispatcher
              ->opendir(InodeNumber::fromThrift(handleEntry.inodeNumber), 0)
              .then([dispatcher, number = handleEntry.handleId](
                        std::shared_ptr<DirHandle> handle) {
                dispatcher->getFileHandles().recordHandle(
                    std::static_pointer_cast<FileHandleBase>(handle), number);
              }));
    } else {
      futures.emplace_back(
          // TODO: we should record the open() flags in the
          // SerializedFileHandleMap so that we can restore
          // the correct flags here.
          dispatcher
              ->open(InodeNumber::fromThrift(handleEntry.inodeNumber), O_RDWR)
              .then([dispatcher, number = handleEntry.handleId](
                        std::shared_ptr<FileHandle> handle) {
                dispatcher->getFileHandles().recordHandle(
                    std::static_pointer_cast<FileHandleBase>(handle), number);
              }));
    }
  }

  FuseChannelData channelData;
  channelData.fd = std::move(info.fuseFD);
  channelData.connInfo = info.connInfo;

  // Start up the fuse workers.
  return folly::collectAllSemiFuture(futures).toUnsafeFuture().then(
      [edenMount, chData = std::move(channelData)]() mutable {
        return edenMount->takeoverFuse(std::move(chData));
      });
}

folly::Future<std::shared_ptr<EdenMount>> EdenServer::mount(
    std::unique_ptr<ClientConfig> initialConfig,
    Optional<TakeoverData::MountInfo>&& optionalTakeover) {
  auto backingStore = getBackingStore(
      initialConfig->getRepoType(), initialConfig->getRepoSource());
  auto objectStore =
      std::make_unique<ObjectStore>(getLocalStore(), backingStore);
  const bool doTakeover = optionalTakeover.hasValue();

  auto edenMount = EdenMount::create(
      std::move(initialConfig), std::move(objectStore), serverState_);

  auto initFuture = edenMount->initialize(
      optionalTakeover ? folly::make_optional(optionalTakeover->inodeMap)
                       : folly::none);
  return initFuture.then(
      [this,
       doTakeover,
       edenMount,
       optionalTakeover = std::move(optionalTakeover)]() mutable {
        addToMountPoints(edenMount);

        return (optionalTakeover ? performTakeoverFuseStart(
                                       edenMount, std::move(*optionalTakeover))
                                 : performFreshFuseStart(edenMount))
            // If an error occurs we want to call mountFinished and throw the
            // error here.  Once the pool is up and running, the finishFuture
            // will ensure that this happens.
            .onError([this, edenMount](folly::exception_wrapper ew) {
              mountFinished(edenMount.get(), folly::none);
              return makeFuture<folly::Unit>(ew);
            })
            .then([edenMount, doTakeover, this] {
              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              auto finishFuture = edenMount->getFuseCompletionFuture().then(
                  [this,
                   edenMount](folly::Try<TakeoverData::MountInfo>&& takeover) {
                    folly::Optional<TakeoverData::MountInfo> optTakeover;
                    if (takeover.hasValue()) {
                      optTakeover = std::move(takeover.value());
                    }
                    mountFinished(edenMount.get(), std::move(optTakeover));
                  });
              // We're deliberately discarding the future here; we don't
              // need to wait for it to finish.
              (void)finishFuture;

              registerStats(edenMount);

              if (!doTakeover) {
                // Perform all of the bind mounts associated with the
                // client.  We don't need to do this for the takeover
                // case as they are already mounted.
                edenMount->performBindMounts();
              }
              return edenMount;
            });
      });
}

Future<Unit> EdenServer::unmount(StringPiece mountPath) {
  try {
    auto future = Future<Unit>::makeEmpty();
    {
      const auto mountPoints = mountPoints_.wlock();
      const auto it = mountPoints->find(mountPath);
      if (it == mountPoints->end()) {
        return makeFuture<Unit>(
            std::out_of_range("no such mount point " + mountPath.str()));
      }
      future = it->second.unmountPromise.getFuture();
    }

    serverState_->getPrivHelper()->fuseUnmount(mountPath);
    return future;
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Failed to perform unmount for \"" << mountPath
              << "\": " << folly::exceptionStr(ex);
    return makeFuture<Unit>(
        folly::exception_wrapper(std::current_exception(), ex));
  }
}

void EdenServer::mountFinished(
    EdenMount* edenMount,
    folly::Optional<TakeoverData::MountInfo> takeover) {
  const auto mountPath = edenMount->getPath().stringPiece();
  XLOG(INFO) << "mount point \"" << mountPath << "\" stopped";
  unregisterStats(edenMount);

  // Erase the EdenMount from our mountPoints_ map
  folly::SharedPromise<Unit> unmountPromise;
  folly::Optional<folly::Promise<TakeoverData::MountInfo>> takeoverPromise;
  {
    const auto mountPoints = mountPoints_.wlock();
    const auto it = mountPoints->find(mountPath);
    CHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    takeoverPromise = std::move(it->second.takeoverPromise);
    mountPoints->erase(it);
  }

  const bool doTakeover = takeoverPromise.hasValue();

  // Shutdown the EdenMount, and fulfill the unmount promise
  // when the shutdown completes
  edenMount->shutdown(doTakeover)
      .then([unmountPromise = std::move(unmountPromise),
             takeoverPromise = std::move(takeoverPromise),
             takeoverData = std::move(takeover)](
                folly::Try<
                    std::tuple<SerializedFileHandleMap, SerializedInodeMap>>&&
                    result) mutable {
        if (takeoverPromise) {
          takeoverPromise.value().setWith([&]() mutable {
            takeoverData.value().fileHandleMap =
                std::move(std::get<0>(result.value()));
            takeoverData.value().inodeMap =
                std::move(std::get<1>(result.value()));
            return std::move(takeoverData.value());
          });
        }
        unmountPromise.setTry(
            folly::makeTryWith([result = std::move(result)]() {
              result.throwIfFailed();
              return Unit{};
            }));
      });
}

EdenServer::MountList EdenServer::getMountPoints() const {
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
  const auto mount = getMountOrNull(mountPath);
  if (!mount) {
    throw EdenError(folly::to<string>(
        "mount point \"", mountPath, "\" is not known to this eden instance"));
  }
  return mount;
}

shared_ptr<EdenMount> EdenServer::getMountOrNull(StringPiece mountPath) const {
  const auto mountPoints = mountPoints_.rlock();
  const auto it = mountPoints->find(mountPath);
  if (it == mountPoints->end()) {
    return nullptr;
  }
  return it->second.edenMount;
}

shared_ptr<BackingStore> EdenServer::getBackingStore(
    StringPiece type,
    StringPiece name) {
  BackingStoreKey key{type.str(), name.str()};
  SYNCHRONIZED(lockedStores, backingStores_) {
    const auto it = lockedStores.find(key);
    if (it != lockedStores.end()) {
      return it->second;
    }

    const auto store = createBackingStore(type, name);
    lockedStores.emplace(key, store);
    return store;
  }

  // Ugh.  The SYNCHRONIZED() macro is super lame.
  // We have to return something here, since the compiler can't figure out
  // that we always return inside SYNCHRONIZED.
  XLOG(FATAL) << "unreached";
}

shared_ptr<BackingStore> EdenServer::createBackingStore(
    StringPiece type,
    StringPiece name) {
  if (type == "null") {
    return make_shared<EmptyBackingStore>();
  } else if (type == "hg") {
    const auto repoPath = realpath(name);
    return make_shared<HgBackingStore>(
        repoPath, localStore_.get(), serverState_->getThreadPool().get());
  } else if (type == "git") {
    const auto repoPath = realpath(name);
    return make_shared<GitBackingStore>(repoPath, localStore_.get());
  } else {
    throw std::domain_error(
        folly::to<string>("unsupported backing store type: ", type));
  }
}

void EdenServer::createThriftServer() {
  server_ = make_shared<ThriftServer>();
  server_->setMaxRequests(FLAGS_thrift_max_requests);
  server_->setNumIOWorkerThreads(FLAGS_thrift_num_workers);
  server_->setEnableCodel(FLAGS_thrift_enable_codel);
  server_->setMinCompressBytes(FLAGS_thrift_min_compress_bytes);

  handler_ = make_shared<EdenServiceHandler>(this);
  server_->setInterface(handler_);

  // Get the path to the thrift socket.
  auto thriftSocketPath = edenDir_ + PathComponentPiece{kThriftSocketName};
  folly::SocketAddress thriftAddress;
  thriftAddress.setFromPath(thriftSocketPath.stringPiece());
  server_->setAddress(thriftAddress);
  serverState_->setSocketPath(thriftSocketPath);

  serverEventHandler_ = make_shared<ThriftServerEventHandler>(this);
  server_->setServerEventHandler(serverEventHandler_);
}

bool EdenServer::acquireEdenLock() {
  const auto lockPath = edenDir_ + PathComponentPiece{kLockFileName};
  lockFile_ = folly::File(lockPath.value(), O_WRONLY | O_CREAT);
  if (!lockFile_.try_lock()) {
    lockFile_.close();
    return false;
  }

  // Write the PID (with a newline) to the lockfile.
  const int fd = lockFile_.fd();
  folly::ftruncateNoInt(fd, /* len */ 0);
  const auto pidContents = folly::to<std::string>(getpid(), "\n");
  folly::writeNoInt(fd, pidContents.data(), pidContents.size());

  return true;
}

void EdenServer::prepareThriftAddress() {
  // If we are serving on a local Unix socket, remove any old socket file
  // that may be left over from a previous instance.
  // We have already acquired the mount point lock at this time, so we know
  // that any existing socket is unused and safe to remove.
  const auto& addr = server_->getAddress();
  if (addr.getFamily() != AF_UNIX) {
    return;
  }
  const int rc = unlink(addr.getPath().c_str());
  if (rc != 0 && errno != ENOENT) {
    // This might happen if we don't have permission to remove the file.
    folly::throwSystemError(
        "unable to remove old Eden thrift socket ", addr.getPath());
  }
}

void EdenServer::stop() const {
  shutdownSubscribers();
  server_->stop();
}

folly::Future<TakeoverData> EdenServer::startTakeoverShutdown() {
  // Make sure we aren't already shutting down, then update our state
  // to indicate that we should perform mount point takeover shutdown
  // once runServer() returns.
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

    // Make a copy of the thrift server socket so we can transfer it to the new
    // edenfs process.  Our local thrift will close its own socket when we stop
    // the server.  The easiest way to avoid completely closing the server
    // socket for now is simply by duplicating the socket to a new fd.
    // We will transfer this duplicated FD to the new edenfs process.
    const int takeoverThriftSocket = dup(server_->getListenSocket());
    folly::checkUnixError(
        takeoverThriftSocket,
        "error duplicating thrift server socket during graceful takeover");
    state->takeoverThriftSocket =
        folly::File{takeoverThriftSocket, /* ownsFd */ true};
  }

  shutdownSubscribers();

  // Stop the thrift server.  We will fulfill takeoverPromise_ once it stops.
  server_->stop();
  return takeoverPromise_.getFuture();
}

void EdenServer::shutdownSubscribers() const {
  // TODO: Set a flag in handler_ to reject future subscription requests.
  // Alternatively, have them seamless transfer through takeovers.

  // If we have any subscription sessions from watchman, we want to shut
  // those down now, otherwise they will block the server_->stop() call
  // below
  XLOG(DBG1) << "cancel all subscribers prior to stopping thrift";
  const auto mountPoints = mountPoints_.wlock();
  for (auto& entry : *mountPoints) {
    auto& info = entry.second;
    info.edenMount->getJournal().cancelAllSubscribers();
  }
}

void EdenServer::flushStatsNow() {
  for (auto& stats : serverState_->getStats().accessAllThreads()) {
    stats.aggregate();
  }
}
} // namespace eden
} // namespace facebook
