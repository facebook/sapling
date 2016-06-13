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
#include <string>
#include <unordered_map>
#include <vector>

namespace apache {
namespace thrift {
class ThriftServer;
}
}

namespace facebook {
namespace eden {

class BackingStore;
class ClientConfig;
class EdenMount;
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
  using MountList = std::vector<std::shared_ptr<EdenMount>>;
  using MountMap = folly::StringKeyedMap<std::shared_ptr<EdenMount>>;

  EdenServer(folly::StringPiece edenDir, folly::StringPiece rocksPath);
  virtual ~EdenServer();

  void run();

  /**
   * Stops this server, which includes the underlying Thrift server.
   */
  void stop() const;

  void mount(
      std::shared_ptr<EdenMount> edenMount,
      std::unique_ptr<ClientConfig> config);

  void unmount(folly::StringPiece mountPath);

  const std::shared_ptr<EdenServiceHandler>& getHandler() const {
    return handler_;
  }
  const std::shared_ptr<apache::thrift::ThriftServer>& getServer() const {
    return server_;
  }

  MountList getMountPoints() const;
  std::shared_ptr<EdenMount> getMount(folly::StringPiece mountPath) const;

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

 private:
  using BackingStoreKey = std::pair<std::string, std::string>;
  using BackingStoreMap =
      std::unordered_map<BackingStoreKey, std::shared_ptr<BackingStore>>;

  // Forbidden copy constructor and assignment operator
  EdenServer(EdenServer const&) = delete;
  EdenServer& operator=(EdenServer const&) = delete;

  std::shared_ptr<BackingStore> createBackingStore(
      folly::StringPiece type,
      folly::StringPiece name);
  void runThriftServer();
  void createThriftServer();
  void acquireEdenLock();
  void prepareThriftAddress();

  // Called when a mount has been unmounted and has stopped.
  void mountFinished(EdenMount* mountPoint);

  std::string edenDir_;
  std::string rocksPath_;
  std::shared_ptr<EdenServiceHandler> handler_;
  std::shared_ptr<apache::thrift::ThriftServer> server_;
  folly::File lockFile_;

  std::shared_ptr<LocalStore> localStore_;
  folly::Synchronized<MountMap> mountPoints_;
  folly::Synchronized<BackingStoreMap> backingStores_;
};
}
} // facebook::eden
