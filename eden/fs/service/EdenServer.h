/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/File.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/Synchronized.h>
#include <folly/experimental/StringKeyedMap.h>
#include <memory>
#include <vector>

namespace apache {
namespace thrift {
class ThriftServer;
}
}
namespace facebook {
namespace eden {
namespace fusell {
class MountPoint;
}
}
}

namespace facebook {
namespace eden {

class EdenServiceHandler;
class LocalStore;

/*
 * EdenServer contains logic for running the Eden main loop.
 *
 * It performs locking to ensure only a single EdenServer instance is running
 * for a particular location, then starts the thrift management server
 * and the fuse session.
 */
class EdenServer {
 public:
  using MountPointList = std::vector<std::shared_ptr<fusell::MountPoint>>;
  using MountPointMap =
      folly::StringKeyedMap<std::shared_ptr<fusell::MountPoint>>;

  EdenServer(folly::StringPiece edenDir, folly::StringPiece rocksPath);
  virtual ~EdenServer();

  void run();

  void mount(std::shared_ptr<fusell::MountPoint> mountPoint);
  void unmount(folly::StringPiece mountPath);

  const std::shared_ptr<EdenServiceHandler>& getHandler() const {
    return handler_;
  }
  const std::shared_ptr<apache::thrift::ThriftServer>& getServer() const {
    return server_;
  }

  MountPointList getMountPoints() const;
  std::shared_ptr<fusell::MountPoint> getMountPoint(
      folly::StringPiece mountPath) const;

  std::shared_ptr<LocalStore> getLocalStore() const {
    return localStore_;
  }

 private:
  // Forbidden copy constructor and assignment operator
  EdenServer(EdenServer const&) = delete;
  EdenServer& operator=(EdenServer const&) = delete;

  void runThriftServer();
  void createThriftServer();
  void acquireEdenLock();
  void prepareThriftAddress();

  // Called when a MountPoint has been unmounted and has stopped.
  void mountFinished(fusell::MountPoint* mountPoint);

  std::string edenDir_;
  std::string rocksPath_;
  std::shared_ptr<EdenServiceHandler> handler_;
  std::shared_ptr<apache::thrift::ThriftServer> server_;
  folly::File lockFile_;

  std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<MountPointMap> mountPoints_;
};
}
} // facebook::eden
