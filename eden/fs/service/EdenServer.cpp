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
#include <folly/experimental/logging/xlog.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp/concurrency/ThreadManager.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>

#include "eden/fs/config/ClientConfig.h"
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
#include "eden/fs/store/ObjectStore.h"
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

DEFINE_string(thrift_address, "", "The address for the thrift server socket");
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
using folly::File;
using folly::Future;
using folly::StringPiece;
using folly::Unit;
using folly::makeFuture;
using std::make_shared;
using std::shared_ptr;
using std::string;
using std::unique_ptr;
using facebook::eden::fusell::FuseChannelData;

namespace {
using namespace facebook::eden;

constexpr StringPiece kLockFileName{"lock"};
constexpr StringPiece kThriftSocketName{"socket"};
constexpr StringPiece kTakeoverSocketName{"takeover"};
constexpr StringPiece kRocksDBPath{"storage/rocks-db"};

folly::SocketAddress getThriftAddress(
    StringPiece argument,
    AbsolutePathPiece edenDir);
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
    AbsolutePathPiece edenDir,
    AbsolutePathPiece etcEdenDir,
    AbsolutePathPiece configPath)
    : edenDir_(edenDir),
      etcEdenDir_(etcEdenDir),
      configPath_(configPath),
      threadPool_(std::make_shared<EdenCPUThreadPool>()) {}

EdenServer::~EdenServer() {}

folly::Future<TakeoverData> EdenServer::takeoverAll() {
  std::vector<Future<TakeoverData::MountInfo>> futures;
  {
    auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      auto& info = entry.second;
      try {
        info.takeoverPromise.emplace();
        auto future = info.takeoverPromise->getFuture();
        info.edenMount->getFuseChannel()->requestSessionExit();
        futures.emplace_back(future.then(
            [edenMount = info.edenMount](FuseChannelData channelData) {
              std::vector<AbsolutePath> bindMounts;
              for (auto& entry : edenMount->getBindMounts()) {
                bindMounts.push_back(entry.pathInMountDir);
              }
              fusell::privilegedFuseTakeoverShutdown(
                  edenMount->getPath().stringPiece());
              return TakeoverData::MountInfo(
                  edenMount->getPath(),
                  edenMount->getConfig()->getClientDirectory(),
                  bindMounts,
                  std::move(channelData.fd),
                  channelData.connInfo,
                  edenMount->getDispatcher()->getFileHandles().serializeMap(),
                  edenMount->getInodeMap()->save());
            }));
      } catch (const std::exception& ex) {
        const auto& mountPath = entry.first;
        XLOG(ERR) << "Failed to perform unmount for \"" << mountPath
                  << "\": " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<TakeoverData::MountInfo>(
            folly::exception_wrapper(std::current_exception(), ex)));
      }
    }
  }

  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).then(
      [](std::vector<folly::Try<TakeoverData::MountInfo>> results) {
        TakeoverData data;
        data.mountPoints.reserve(results.size());
        for (auto& result : results) {
          // Note: .value() will throw if there was a problem;
          // we want this to happen so we don't do anything
          // special to catch it here.
          data.mountPoints.emplace_back(std::move(result.value()));
        }
        return data;
      });
}

folly::Future<Unit> EdenServer::unmountAll() {
  std::vector<Future<Unit>> futures;
  {
    auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      auto& info = entry.second;
      // Make sure we hold a reference to the edenMount!
      auto edenMount = info.edenMount;

      try {
        fusell::privilegedFuseUnmount(mountPath);
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Failed to perform unmount for \"" << mountPath
                  << "\": " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<Unit>(
            folly::exception_wrapper(std::current_exception(), ex)));
        continue;
      }
      futures.push_back(info.unmountPromise.getFuture());
    }
  }

  // Use collectAll() rather than collect() to wait for all of the unmounts
  // to complete, and only check for errors once everything has finished.
  return folly::collectAll(futures).then(
      [](const std::vector<folly::Try<Unit>>& results) {
        for (const auto& result : results) {
          result.throwIfFailed();
        }
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
    auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      roots.emplace_back(entry.second.edenMount->getRootInode());
    }
  }

  if (!roots.empty()) {
    XLOG(INFO) << "UnloadInodeScheduler Unloading Free Inodes";
    auto serviceData = stats::ServiceData::get();

    uint64_t totalUnloaded = serviceData->getCounter(kPeriodicUnloadCounterKey);
    for (auto& rootInode : roots) {
      totalUnloaded += rootInode->unloadChildrenNow(
          std::chrono::minutes(FLAGS_unload_age_minutes));
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
  auto takeoverPath = edenDir_ + PathComponentPiece{"takeover"};
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

  XLOG(DBG2) << "opening local RocksDB store";
  auto rocksPath = edenDir_ + RelativePathPiece{kRocksDBPath};
  localStore_ = make_shared<LocalStore>(rocksPath);
  XLOG(DBG2) << "done opening local RocksDB store";

  // Start listening for graceful takeover requests
  takeoverServer_.reset(
      new TakeoverServer(getMainEventBase(), takeoverPath, this));
  takeoverServer_->start();

  // Remount existing mount points
  // if doingTakeover is true, use the mounts received in TakeoverData
  if (doingTakeover) {
    for (auto& info : takeoverData.mountPoints) {
      auto stateDirectory = info.stateDirectory;
      try {
        takeoverMount(std::move(info)).get();
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
    for (auto& client : dirs.items()) {
      MountInfo mountInfo;
      mountInfo.mountPoint = client.first.c_str();
      auto edenClientPath = edenDir_ + PathComponent("clients") +
          PathComponent(client.second.c_str());
      mountInfo.edenClientPath = edenClientPath.stringPiece().str();
      try {
        mount(mountInfo).get();
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
  auto takeoverPath = edenDir_ + PathComponentPiece{kTakeoverSocketName};
  takeoverServer_.reset(
      new TakeoverServer(getMainEventBase(), takeoverPath, this));
  takeoverServer_->start();

  // Run the thrift server
  state_.wlock()->state = State::RUNNING;
  runServer(*this);

  bool takeover;
  folly::File thriftSocket;
  {
    auto state = state_.wlock();
    takeover = state->takeoverShutdown;
    if (takeover) {
      thriftSocket = std::move(state->takeoverThriftSocket);
    }
    state->state = State::SHUTTING_DOWN;
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
  return takeoverAll().then(
      [this, socket = std::move(thriftSocket)](TakeoverData data) mutable {

        // Destroy the local store and backing stores.
        // We shouldn't access the local store any more after giving up our
        // lock, and we need to close it to release its lock before the new
        // edenfs process tries to open it.
        backingStores_.wlock()->clear();
        // Explicit close the LocalStore before we reset our pointer, to ensure
        // we release the RocksDB lock.  Since this is managed with a shared_ptr
        // it is somewhat hard to confirm if we really have the last reference
        // to it.
        localStore_->close();
        localStore_.reset();

        // Stop the privhelper process.
        shutdownPrivhelper();

        data.lockFile = std::move(lockFile_);
        auto future = data.takeoverComplete.getFuture();
        data.thriftSocket = std::move(socket);

        takeoverPromise_.setValue(std::move(data));
        return future;
      });
}

Future<Unit> EdenServer::performNormalShutdown() {
  takeoverServer_.reset();

  // Clean up all the server mount points before shutting down the privhelper.
  auto shutdownFuture = unmountAll();

  shutdownPrivhelper();

  return shutdownFuture;
}

void EdenServer::shutdownPrivhelper() {
  // Explicitly stop the privhelper process so we can verify that it
  // exits normally.
  auto privhelperExitCode = fusell::stopPrivHelper();
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
    auto mountPoints = mountPoints_.wlock();
    auto ret = mountPoints->emplace(mountPath, EdenMountInfo(edenMount));
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

folly::Future<std::shared_ptr<EdenMount>> EdenServer::takeoverMount(
    TakeoverData::MountInfo&& info) {
  auto config = ClientConfig::loadFromClientDirectory(
      info.mountPath, info.stateDirectory);

  auto repoType = config->getRepoType();
  auto backingStore = getBackingStore(repoType, config->getRepoSource());
  auto objectStore =
      std::make_unique<ObjectStore>(getLocalStore(), backingStore);

  return EdenMount::create(
             std::move(config),
             std::move(objectStore),
             getSocketPath(),
             getStats(),
             std::make_shared<UnixClock>())
      .then([this, info = std::move(info)](
                std::shared_ptr<EdenMount> edenMount) mutable {
        // Load InodeBase objects for any materialized files in this mount point
        // before we start mounting.
        auto rootInode = edenMount->getRootInode();

        edenMount->getInodeMap()->load(info.inodeMap);
        // TODO: open file handles for each entry in info.fileHandleMap

        return rootInode->loadMaterializedChildren()
            .then([this, edenMount, info = std::move(info)](
                      folly::Try<folly::Unit> t) mutable {
              (void)t; // We're explicitly ignoring possible failure in
                       // loadMaterializedChildren, but only because we were
                       // previously using .wait() on the future.  We could
                       // just let potential errors propagate.

              XLOG(ERR) << "addToMountPoints";
              addToMountPoints(edenMount);
              XLOG(ERR) << "privilegedFuseTakeoverStartup";
              std::vector<std::string> bindMounts;
              for (const auto& bindMount : info.bindMounts) {
                bindMounts.emplace_back(bindMount.value());
              }
              fusell::privilegedFuseTakeoverStartup(
                  info.mountPath.stringPiece(), bindMounts);

              XLOG(ERR) << "takeoverFuse";

              FuseChannelData channelData;
              channelData.fd = std::move(info.fuseFD);
              channelData.connInfo = info.connInfo;

              // Start up the fuse workers.
              return edenMount->startFuse(
                  getMainEventBase(), threadPool_, std::move(channelData));
            })
            // If an error occurs we want to call mountFinished and throw the
            // error here.  Once the pool is up and running, the finishFuture
            // will ensure that this happens.
            .onError([this, edenMount](folly::exception_wrapper ew) {
              XLOG(ERR) << "failed to takeover " << ew;
              mountFinished(edenMount.get(), FuseChannelData{});
              return makeFuture<folly::Unit>(ew);
            })
            // Explicitly move the remainder of processing to a utility
            // thread; we're likely to reach this point in the context of
            // a fuse mount thread prior to it responding to the mount
            // initiation request from the kernel, so if we were to block
            // here, that would lead to deadlock.  In addition, if we were
            // to run this via mainEventBase_ we could also deadlock
            // during started when remounting configured mounts.
            .via(threadPool_.get())
            .then([edenMount, this] {
              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              XLOG(ERR) << "this bit";
              auto finishFuture = edenMount->getFuseCompletionFuture().then(
                  [this, edenMount](folly::Try<FuseChannelData>&& fuseFd) {
                    mountFinished(
                        edenMount.get(),
                        fuseFd.hasValue() ? std::move(fuseFd).value()
                                          : FuseChannelData{});
                  });
              // We're deliberately discarding the future here; we don't
              // need to wait for it to finish.
              (void)finishFuture;

              registerStats(edenMount);

              // The bind mounts were performed in mount() in our ancestor,
              // so we don't need to do that here now.

              return edenMount;
            });
      });
}

folly::Future<std::shared_ptr<EdenMount>> EdenServer::mount(
    const MountInfo& info) {
  auto initialConfig = ClientConfig::loadFromClientDirectory(
      AbsolutePathPiece{info.mountPoint},
      AbsolutePathPiece{info.edenClientPath});

  auto repoType = initialConfig->getRepoType();
  auto backingStore = getBackingStore(repoType, initialConfig->getRepoSource());
  auto objectStore =
      std::make_unique<ObjectStore>(getLocalStore(), backingStore);

  return EdenMount::create(
             std::move(initialConfig),
             std::move(objectStore),
             getSocketPath(),
             getStats(),
             std::make_shared<UnixClock>())
      .then([this](std::shared_ptr<EdenMount> edenMount) {
        // Load InodeBase objects for any materialized files in this mount point
        // before we start mounting.
        auto rootInode = edenMount->getRootInode();
        return rootInode->loadMaterializedChildren()
            .then([this, edenMount](folly::Try<folly::Unit> t) {
              (void)t; // We're explicitly ignoring possible failure in
                       // loadMaterializedChildren, but only because we were
                       // previously using .wait() on the future.  We could
                       // just let potential errors propagate.

              addToMountPoints(edenMount);

              // Start up the fuse workers.
              return edenMount->startFuse(
                  getMainEventBase(), threadPool_, folly::none);
            })
            // If an error occurs we want to call mountFinished and throw the
            // error here.  Once the pool is up and running, the finishFuture
            // will ensure that this happens.
            .onError([this, edenMount](folly::exception_wrapper ew) {
              mountFinished(edenMount.get(), FuseChannelData{});
              return makeFuture<folly::Unit>(ew);
            })
            // Explicitly move the remainder of processing to a utility
            // thread; we're likely to reach this point in the context of
            // a fuse mount thread prior to it responding to the mount
            // initiation request from the kernel, so if we were to block
            // here, that would lead to deadlock.  In addition, if we were
            // to run this via mainEventBase_ we could also deadlock
            // during started when remounting configured mounts.
            .via(threadPool_.get())
            .then([edenMount, this] {
              // Now that we've started the workers, arrange to call
              // mountFinished once the pool is torn down.
              auto finishFuture = edenMount->getFuseCompletionFuture().then(
                  [this, edenMount](folly::Try<FuseChannelData>&& fuseFd) {
                    mountFinished(
                        edenMount.get(),
                        fuseFd.hasValue() ? std::move(fuseFd).value()
                                          : FuseChannelData{});
                  });
              // We're deliberately discarding the future here; we don't
              // need to wait for it to finish.
              (void)finishFuture;

              registerStats(edenMount);

              // Perform all of the bind mounts associated with the
              // client.
              edenMount->performBindMounts();
              return edenMount;
            });
      });
}

Future<Unit> EdenServer::unmount(StringPiece mountPath) {
  try {
    auto future = Future<Unit>::makeEmpty();
    {
      auto mountPoints = mountPoints_.wlock();
      auto it = mountPoints->find(mountPath);
      if (it == mountPoints->end()) {
        return makeFuture<Unit>(
            std::out_of_range("no such mount point " + mountPath.str()));
      }
      future = it->second.unmountPromise.getFuture();
    }

    fusell::privilegedFuseUnmount(mountPath);
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
    FuseChannelData channelData) {
  auto mountPath = edenMount->getPath().stringPiece();
  XLOG(INFO) << "mount point \"" << mountPath << "\" stopped";
  unregisterStats(edenMount);

  // Erase the EdenMount from our mountPoints_ map
  folly::SharedPromise<Unit> unmountPromise;
  folly::Optional<folly::Promise<FuseChannelData>> takeoverPromise;
  {
    auto mountPoints = mountPoints_.wlock();
    auto it = mountPoints->find(mountPath);
    CHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    takeoverPromise = std::move(it->second.takeoverPromise);
    mountPoints->erase(it);
  }

  // Shutdown the EdenMount, and fulfill the unmount promise
  // when the shutdown completes
  edenMount->shutdown().then([unmountPromise = std::move(unmountPromise),
                              takeoverPromise = std::move(takeoverPromise),
                              channelData = std::move(channelData)](
                                 folly::Try<folly::Unit> result) mutable {
    if (takeoverPromise) {
      takeoverPromise.value().setValue(std::move(channelData));
    }
    unmountPromise.setTry(std::move(result));
  });
}

EdenServer::MountList EdenServer::getMountPoints() const {
  MountList results;
  {
    auto mountPoints = mountPoints_.rlock();
    for (const auto& entry : *mountPoints) {
      results.emplace_back(entry.second.edenMount);
    }
  }
  return results;
}

shared_ptr<EdenMount> EdenServer::getMount(StringPiece mountPath) const {
  auto mount = getMountOrNull(mountPath);
  if (!mount) {
    throw EdenError(folly::to<string>(
        "mount point \"", mountPath, "\" is not known to this eden instance"));
  }
  return mount;
}

shared_ptr<EdenMount> EdenServer::getMountOrNull(StringPiece mountPath) const {
  auto mountPoints = mountPoints_.rlock();
  auto it = mountPoints->find(mountPath);
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
    auto it = lockedStores.find(key);
    if (it != lockedStores.end()) {
      return it->second;
    }

    auto store = createBackingStore(type, name);
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
    auto repoPath = realpath(name);
    return make_shared<HgBackingStore>(
        repoPath, localStore_.get(), threadPool_.get());
  } else if (type == "git") {
    auto repoPath = realpath(name);
    return make_shared<GitBackingStore>(repoPath, localStore_.get());
  } else {
    throw std::domain_error(
        folly::to<string>("unsupported backing store type: ", type));
  }
}

void EdenServer::createThriftServer() {
  auto address = getThriftAddress(FLAGS_thrift_address, edenDir_);

  server_ = make_shared<ThriftServer>();
  server_->setMaxRequests(FLAGS_thrift_max_requests);
  server_->setNumIOWorkerThreads(FLAGS_thrift_num_workers);
  server_->setEnableCodel(FLAGS_thrift_enable_codel);
  server_->setMinCompressBytes(FLAGS_thrift_min_compress_bytes);

  handler_ = make_shared<EdenServiceHandler>(this);
  server_->setInterface(handler_);
  server_->setAddress(address);

  serverEventHandler_ = make_shared<ThriftServerEventHandler>(this);
  server_->setServerEventHandler(serverEventHandler_);
}

bool EdenServer::acquireEdenLock() {
  auto lockPath = edenDir_ + PathComponentPiece{kLockFileName};
  lockFile_ = folly::File(lockPath.value(), O_WRONLY | O_CREAT);
  if (!lockFile_.try_lock()) {
    lockFile_.close();
    return false;
  }

  // Write the PID (with a newline) to the lockfile.
  int fd = lockFile_.fd();
  folly::ftruncateNoInt(fd, /* len */ 0);
  auto pidContents = folly::to<std::string>(getpid(), "\n");
  folly::writeNoInt(fd, pidContents.data(), pidContents.size());

  return true;
}

AbsolutePath EdenServer::getSocketPath() const {
  const auto& addr = server_->getAddress();
  CHECK_EQ(addr.getFamily(), AF_UNIX);
  // Need to make a copy rather than a Piece here because getPath returns
  // a temporary std::string instance.
  return AbsolutePath{addr.getPath()};
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
  int rc = unlink(addr.getPath().c_str());
  if (rc != 0 && errno != ENOENT) {
    // This might happen if we don't have permission to remove the file.
    folly::throwSystemError(
        "unable to remove old Eden thrift socket ", addr.getPath());
  }
}

void EdenServer::stop() const {
  server_->stop();
}

folly::Future<TakeoverData> EdenServer::startTakeoverShutdown() {
  // Make sure we aren't already shutting down, then update our state
  // to indicate that we should perform mount point takeover shutdown
  // once runServer() returns.
  {
    auto state = state_.wlock();
    if (state->state != State::RUNNING) {
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
    int takeoverThriftSocket = dup(server_->getListenSocket());
    folly::checkUnixError(
        takeoverThriftSocket,
        "error duplicating thrift server socket during graceful takeover");
    state->takeoverThriftSocket =
        folly::File{takeoverThriftSocket, /* ownsFd */ true};
  }

  // Stop the thrift server.  We will fulfill takeoverPromise_ once it stops.
  server_->stop();
  return takeoverPromise_.getFuture();
}

void EdenServer::flushStatsNow() const {
  for (auto& stats : edenStats_.accessAllThreads()) {
    stats.aggregate();
  }
}
} // namespace eden
} // namespace facebook

namespace {

/*
 * Parse the --thrift_address argument, and return a SocketAddress object
 */
folly::SocketAddress getThriftAddress(
    StringPiece argument,
    AbsolutePathPiece edenDir) {
  folly::SocketAddress addr;

  // If the argument is empty, default to a Unix socket placed next
  // to the mount point
  if (argument.empty()) {
    auto socketPath = edenDir + PathComponentPiece{kThriftSocketName};
    addr.setFromPath(socketPath.stringPiece());
    return addr;
  }

  // Check to see if the argument looks like a port number
  uint16_t port;
  bool validPort{false};
  try {
    port = folly::to<uint16_t>(argument);
    validPort = true;
  } catch (const std::range_error& ex) {
    // validPort = false
  }
  if (validPort) {
    addr.setFromLocalPort(port);
    return addr;
  }

  // TODO: also support IPv4:PORT or [IPv6]:PORT

  // Otherwise assume the address refers to a local unix socket path
  addr.setFromPath(argument);
  return addr;
}

} // unnamed namespace
