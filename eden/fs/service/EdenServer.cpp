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

#include <boost/filesystem/operations.hpp>
#include <boost/filesystem/path.hpp>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <wangle/concurrent/CPUThreadPoolExecutor.h>
#include <wangle/concurrent/GlobalExecutor.h>

#include "eden/fs/config/ClientConfig.h"
#include "eden/fs/fuse/MountPoint.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/inodes/Dirstate.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/service/EdenServiceHandler.h"
#include "eden/fs/store/EmptyBackingStore.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/store/git/GitBackingStore.h"
#include "eden/fs/store/hg/HgBackingStore.h"

DEFINE_bool(debug, false, "run fuse in debug mode");

DEFINE_int32(num_eden_threads, 12, "the number of eden CPU worker threads");

DEFINE_string(thrift_address, "", "The address for the thrift server socket");
DEFINE_int32(thrift_num_workers, 2, "The number of thrift worker threads");
DEFINE_int32(thrift_max_conns, 100, "Maximum number of thrift connections");
DEFINE_int32(
    thrift_max_requests,
    1000,
    "Maximum number of active thrift requests");
DEFINE_bool(thrift_enable_codel, true, "Enable Codel queuing timeout");
DEFINE_int32(thrift_queue_len, 100, "Maximum number of unprocessed messages");
DEFINE_int32(
    thrift_min_compress_bytes,
    200,
    "Minimum response compression size");
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
using folly::Future;
using folly::makeFuture;
using folly::StringPiece;
using folly::Unit;
using std::make_shared;
using std::shared_ptr;
using std::string;
using std::unique_ptr;

namespace {
folly::SocketAddress getThriftAddress(
    StringPiece argument,
    StringPiece edenDir);
std::string getPathToUnixDomainSocket(StringPiece edenDir);
}

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
    AbsolutePathPiece configPath,
    AbsolutePathPiece rocksPath)
    : edenDir_(edenDir),
      etcEdenDir_(etcEdenDir),
      configPath_(configPath),
      rocksPath_(rocksPath) {}

EdenServer::~EdenServer() {
  shutdown();
}

folly::Future<Unit> EdenServer::unmountAll() {
  std::vector<Future<Unit>> futures;
  {
    auto mountPoints = mountPoints_.wlock();
    for (auto& entry : *mountPoints) {
      const auto& mountPath = entry.first;
      try {
        fusell::privilegedFuseUnmount(mountPath);
      } catch (const std::exception& ex) {
        XLOG(ERR) << "Failed to perform unmount for \"" << mountPath
                  << "\": " << folly::exceptionStr(ex);
        futures.push_back(makeFuture<Unit>(
            folly::exception_wrapper(std::current_exception(), ex)));
        continue;
      }
      futures.push_back(entry.second.unmountPromise.getFuture());
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

void EdenServer::prepare() {
  acquireEdenLock();
  // Store a pointer to the EventBase that will be used to drive
  // the main thread.  The runServer() code will end up driving this EventBase.
  mainEventBase_ = folly::EventBaseManager::get()->getEventBase();
  createThriftServer();

  localStore_ = make_shared<LocalStore>(rocksPath_);
  functionScheduler_ = make_shared<folly::FunctionScheduler>();
  functionScheduler_->setThreadName("EdenFuncSched");
  functionScheduler_->start();

  // Start stats aggregation
  functionScheduler_->addFunction(
      [this] { flushStatsNow(); }, std::chrono::seconds(1));

  // Set the ServiceData counter for tracking number of inodes unloaded by
  // periodic job for unloading inodes to zero on EdenServer start.
  stats::ServiceData::get()->setCounter(kPeriodicUnloadCounterKey, 0);

  auto pool =
      make_shared<wangle::CPUThreadPoolExecutor>(FLAGS_num_eden_threads);
  wangle::setCPUExecutor(pool);

  reloadConfig();

  // Remount existing mount points
  folly::dynamic dirs = folly::dynamic::object();
  try {
    dirs = ClientConfig::loadClientDirectoryMap(edenDir_);
  } catch (const std::exception& ex) {
    XLOG(ERR) << "Could not parse config.json file: " << ex.what()
              << " Skipping remount step.";
  }
  for (auto& client : dirs.items()) {
    auto mountInfo = std::make_unique<MountInfo>();
    mountInfo->mountPoint = client.first.c_str();
    auto edenClientPath = edenDir_ + PathComponent("clients") +
        PathComponent(client.second.c_str());
    mountInfo->edenClientPath = edenClientPath.stringPiece().str();
    try {
      handler_->mount(std::move(mountInfo));
    } catch (const std::exception& ex) {
      XLOG(ERR) << "Failed to perform remount for " << client.first.c_str()
                << ": " << ex.what();
    }
  }
  prepareThriftAddress();
}

// Defined separately in RunServer.cpp
void runServer(const EdenServer& server);

void EdenServer::run() {
  // Acquire the eden lock, prepare the thrift server, and start our mounts
  prepare();

  // Run the thrift server
  runServer(*this);

  // Clean up all the server mount points before shutting down the privhelper.
  // This is made a little bit more complicated because we're running on
  // the main event base thread here, and the unmount handling relies on
  // scheduling the unmount to run in our thread; we can't simply block
  // on the future returned from unmountAll() as that would prevent those
  // actions from completing, so we perform a somewhat inelegant polling
  // loop on both the eventBase and the future.
  auto unmounted = unmountAll();

  CHECK_EQ(mainEventBase_, folly::EventBaseManager::get()->getEventBase());
  while (!unmounted.isReady()) {
    mainEventBase_->loopOnce();
  }
  unmounted.get();

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

void EdenServer::mount(shared_ptr<EdenMount> edenMount) {
  // Add the mount point to mountPoints_.
  // This also makes sure we don't have this path mounted already
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

  auto onFinish = [this, edenMount]() { this->mountFinished(edenMount.get()); };
  try {
    edenMount->getMountPoint()->start(
        getMainEventBase(), onFinish, FLAGS_debug);
  } catch (...) {
    // If we fail to start the mount point, call mountFinished()
    // to make sure it gets removed from mountPoints_.
    //
    // Note that we can't perform this clean-up using SCOPE_FAIL() for now, due
    // to a bug in some versions of gcc:
    // https://gcc.gnu.org/bugzilla/show_bug.cgi?id=62258
    this->mountFinished(edenMount.get());
    throw;
  }

  // Adding function for the newly added mountpoint to Schedule
  // a periodic job to unload unused inodes based on the last access time.
  // currently Eden doesnot have accurate timestamp tracking for inodes, so
  // using unloadChildrenNow just to validate the behaviour. We will have to
  // modify current unloadChildrenNow function to unload inodes based on the
  // last access time.
  if (FLAGS_unload_interval_hours > 0) {
    functionScheduler_->addFunction(
        [edenMount] {
          auto rootInode = (edenMount.get())->getRootInode();
          XLOG(INFO) << "UnloadInodeScheduler Unloading Free Inodes";
          auto unloadCount = rootInode->unloadChildrenNow(
              std::chrono::minutes(FLAGS_unload_age_minutes));
          unloadCount +=
              stats::ServiceData::get()->getCounter(kPeriodicUnloadCounterKey);
          stats::ServiceData::get()->setCounter(
              kPeriodicUnloadCounterKey, unloadCount);
        },
        std::chrono::hours(FLAGS_unload_interval_hours),
        getPeriodicUnloadFunctionName(edenMount.get()),
        std::chrono::minutes(FLAGS_start_delay_minutes));
  }

  // TODO(T21262294): We will have to implement a mechanism to get the counter
  // names for a mount point.
  // Register callback for getting Loaded inodes in the memory for a mountPoint.
  stats::ServiceData::get()->getDynamicCounters()->registerCallback(
      edenMount->getCounterName(CounterName::LOADED), [edenMount] {
        return edenMount.get()->getInodeMap()->getLoadedInodeCount();
      });
  // Register callback for getting Unloaded inodes in the memory for a
  // mountpoint
  stats::ServiceData::get()->getDynamicCounters()->registerCallback(
      edenMount->getCounterName(CounterName::UNLOADED), [edenMount] {
        return edenMount.get()->getInodeMap()->getUnloadedInodeCount();
      });

  // Perform all of the bind mounts associated with the client.
  for (auto& bindMount : edenMount->getBindMounts()) {
    auto pathInMountDir = bindMount.pathInMountDir;
    try {
      // If pathInMountDir does not exist, then it must be created before the
      // bind mount is performed.
      boost::system::error_code errorCode;
      boost::filesystem::path mountDir = pathInMountDir.c_str();
      boost::filesystem::create_directories(mountDir, errorCode);

      fusell::privilegedBindMount(
          bindMount.pathInClientDir.c_str(), pathInMountDir.c_str());
    } catch (...) {
      // Consider recording all failed bind mounts in a way that can be
      // communicated back to the caller in a structured way.
      XLOG(ERR) << "Failed to perform bind mount for "
                << pathInMountDir.stringPiece() << ".";
    }
  }
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

void EdenServer::mountFinished(EdenMount* edenMount) {
  auto mountPath = edenMount->getPath().stringPiece();
  XLOG(INFO) << "mount point \"" << mountPath << "\" stopped";
  functionScheduler_->cancelFunctionAndWait(
      getPeriodicUnloadFunctionName(edenMount));
  stats::ServiceData::get()->getDynamicCounters()->unregisterCallback(
      edenMount->getCounterName(CounterName::LOADED));
  stats::ServiceData::get()->getDynamicCounters()->unregisterCallback(
      edenMount->getCounterName(CounterName::UNLOADED));

  // Erase the EdenMount from our mountPoints_ map
  folly::SharedPromise<Unit> unmountPromise;
  {
    auto mountPoints = mountPoints_.wlock();
    auto it = mountPoints->find(mountPath);
    CHECK(it != mountPoints->end());
    unmountPromise = std::move(it->second.unmountPromise);
    mountPoints->erase(it);
  }

  // Shutdown the EdenMount, and fulfill the unmount promise
  // when the shutdown completes
  edenMount->shutdown()
      .then([unmountPromise = std::move(unmountPromise)]() mutable {
        unmountPromise.setValue();
      });
}

string EdenServer::getPeriodicUnloadFunctionName(const EdenMount* mount) {
  return folly::to<string>("unload:", mount->getPath().stringPiece());
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

void EdenServer::reloadConfig() {
  *configData_.wlock() = make_shared<ConfigData>(
      ClientConfig::loadConfigData(etcEdenDir_.piece(), configPath_.piece()));
}

shared_ptr<EdenServer::ConfigData> EdenServer::getConfig() {
  return *configData_.rlock();
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
    return make_shared<HgBackingStore>(repoPath, localStore_.get());
  } else if (type == "git") {
    auto repoPath = realpath(name);
    return make_shared<GitBackingStore>(repoPath, localStore_.get());
  } else {
    throw std::domain_error(
        folly::to<string>("unsupported backing store type: ", type));
  }
}

void EdenServer::createThriftServer() {
  auto address = getThriftAddress(FLAGS_thrift_address, edenDir_.stringPiece());

  server_ = make_shared<ThriftServer>();
  server_->setMaxConnections(FLAGS_thrift_max_conns);
  server_->setMaxRequests(FLAGS_thrift_max_requests);
  server_->setNumIOWorkerThreads(FLAGS_thrift_num_workers);
  server_->setEnableCodel(FLAGS_thrift_enable_codel);
  server_->setMaxNumPendingConnectionsPerWorker(FLAGS_thrift_queue_len);
  server_->setMinCompressBytes(FLAGS_thrift_min_compress_bytes);

  handler_ = make_shared<EdenServiceHandler>(this);
  server_->setInterface(handler_);
  server_->setAddress(address);

  serverEventHandler_ = make_shared<ThriftServerEventHandler>(this);
  server_->setServerEventHandler(serverEventHandler_);
}

void EdenServer::acquireEdenLock() {
  boost::filesystem::path edenPath{edenDir_.stringPiece().str()};
  boost::filesystem::path lockPath = edenPath / "lock";
  lockFile_ = folly::File(lockPath.string(), O_WRONLY | O_CREAT);
  if (!lockFile_.try_lock()) {
    throw std::runtime_error(
        "another instance of Eden appears to be running for " +
        edenDir_.stringPiece().str());
  }
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

void EdenServer::shutdown() {
  unmountAll().get();
  functionScheduler_->shutdown();
}

void EdenServer::flushStatsNow() const {
  for (auto& stats : edenStats_.accessAllThreads()) {
    stats.aggregate();
  }
}
}
} // facebook::eden

namespace {

/*
 * Parse the --thrift_address argument, and return a SocketAddress object
 */
folly::SocketAddress getThriftAddress(
    StringPiece argument,
    StringPiece edenDir) {
  folly::SocketAddress addr;

  // If the argument is empty, default to a Unix socket placed next
  // to the mount point
  if (argument.empty()) {
    auto socketPath = getPathToUnixDomainSocket(edenDir);
    addr.setFromPath(socketPath);
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

std::string getPathToUnixDomainSocket(StringPiece edenDir) {
  boost::filesystem::path edenPath{edenDir.str()};
  boost::filesystem::path socketPath = edenPath / "socket";
  return socketPath.string();
}

} // unnamed namespace
