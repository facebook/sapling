/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "EdenServer.h"

#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <wangle/concurrent/CPUThreadPoolExecutor.h>
#include <wangle/concurrent/GlobalExecutor.h>
#include <boost/filesystem/path.hpp>

#include "EdenServiceHandler.h"
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fuse/MountPoint.h"

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

using apache::thrift::ThriftServer;
using folly::StringPiece;
using std::string;

namespace {
folly::SocketAddress getThriftAddress(
    StringPiece argument,
    StringPiece edenDir);
std::string getPathToUnixDomainSocket(StringPiece edenDir);
}

namespace facebook {
namespace eden {

EdenServer::EdenServer(StringPiece edenDir, StringPiece rocksPath)
    : edenDir_(edenDir.str()), rocksPath_(rocksPath.str()) {}

EdenServer::~EdenServer() {}

void EdenServer::run() {
  acquireEdenLock();
  createThriftServer();
  localStore_ = std::make_shared<LocalStore>(rocksPath_);

  auto pool =
      std::make_shared<wangle::CPUThreadPoolExecutor>(FLAGS_num_eden_threads);
  wangle::setCPUExecutor(pool);

  prepareThriftAddress();
  runThriftServer();
}

void EdenServer::mount(std::shared_ptr<EdenMount> edenMount) {
  // Add the mount point to mountPoints_.
  // This also makes sure we don't have this path mounted already
  auto mountPath = edenMount->getPath().stringPiece();
  SYNCHRONIZED(mp, mountPoints_) {
    auto ret = mp.emplace(mountPath, edenMount);
    if (!ret.second) {
      // This mount point already exists.
      throw EdenError(folly::to<string>(
          "mount point \"", mountPath, "\" is already mounted"));
    }
  }

  auto onFinish = [this, edenMount]() { this->mountFinished(edenMount.get()); };
  try {
    edenMount->getMountPoint()->start(FLAGS_debug, onFinish);
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
}

void EdenServer::mountFinished(EdenMount* edenMount) {
  auto mountPath = edenMount->getPath().stringPiece();
  LOG(INFO) << "mount point \"" << mountPath << "\" stopped";
  SYNCHRONIZED(mp, mountPoints_) {
    auto numErased = mp.erase(mountPath);
    CHECK_EQ(numErased, 1);
  }
}

void EdenServer::unmount(StringPiece mountPath) {
  // TODO
}

EdenServer::MountList EdenServer::getMountPoints() const {
  MountList results;
  SYNCHRONIZED(mp, mountPoints_) {
    for (const auto& entry : mp) {
      results.emplace_back(entry.second);
    }
  }
  return results;
}

std::shared_ptr<EdenMount> EdenServer::getMount(StringPiece mountPath) const {
  SYNCHRONIZED(mp, mountPoints_) {
    auto it = mp.find(mountPath);
    if (it == mp.end()) {
      return nullptr;
    }
    return it->second;
  }
  // Not reached, but the SYNCHRONIZED macro sucks, and we have to put
  // something here to avoid compiler warnings.
  return nullptr;
}

void EdenServer::createThriftServer() {
  auto address = getThriftAddress(FLAGS_thrift_address, edenDir_);

  server_ = std::make_shared<ThriftServer>();
  server_->setMaxConnections(FLAGS_thrift_max_conns);
  server_->setMaxRequests(FLAGS_thrift_max_requests);
  server_->setNWorkerThreads(FLAGS_thrift_num_workers);
  server_->setEnableCodel(FLAGS_thrift_enable_codel);
  server_->setMaxNumPendingConnectionsPerWorker(FLAGS_thrift_queue_len);
  server_->setMinCompressBytes(FLAGS_thrift_min_compress_bytes);

  handler_ = std::make_shared<EdenServiceHandler>(this);
  server_->setInterface(handler_);
  server_->setAddress(address);
}

void EdenServer::acquireEdenLock() {
  boost::filesystem::path edenPath{edenDir_};
  boost::filesystem::path lockPath = edenPath / "lock";
  lockFile_ = folly::File(lockPath.string(), O_WRONLY | O_CREAT);
  if (!lockFile_.try_lock()) {
    throw std::runtime_error(
        "another instance of Eden appears to be running for " + edenDir_);
  }
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
