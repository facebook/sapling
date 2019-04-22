/*
 *  Copyright (c) 2019-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <sysexits.h>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/config/ReloadableConfig.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/EdenStateDir.h"
#include "eden/fs/store/RocksDbLocalStore.h"
#include "eden/fs/utils/FaultInjector.h"

using namespace facebook::eden;

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

namespace {
class StoreConfig : public ReloadableConfig {
 public:
  explicit StoreConfig(std::shared_ptr<EdenConfig> config)
      : config_(std::move(config)) {}

  std::shared_ptr<const EdenConfig> getEdenConfig(
      bool /* skipUpdate */ = false) override {
    // We don't ever bother checking for an update for now.
    // We don't expect to be a long-lived process.
    return config_;
  }

 private:
  std::shared_ptr<EdenConfig> config_;
};
} // namespace

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  auto userInfo = UserInfo::lookup();
  std::shared_ptr<EdenConfig> config;
  try {
    config = getEdenConfig(userInfo);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return EX_SOFTWARE;
  }

  XLOG(INFO) << "Using Eden directory: " << config->getEdenDir();
  EdenStateDir edenDir(config->getEdenDir());
  if (!edenDir.acquireLock()) {
    fprintf(stderr, "error: failed to acquire the Eden lock\n");
    fprintf(stderr, "This utility cannot be used while edenfs is running.\n");
    return EX_SOFTWARE;
  }

  FaultInjector faultInjector(/*enabled=*/false);
  folly::stop_watch<std::chrono::milliseconds> watch;
  const auto rocksPath = edenDir.getPath() + "storage/rocks-db"_relpath;
  ensureDirectoryExists(rocksPath);
  auto localStore = std::make_unique<RocksDbLocalStore>(
      rocksPath, &faultInjector, std::make_shared<StoreConfig>(config));
  XLOG(INFO) << "Opened RocksDB store in " << (watch.elapsed().count() / 1000.0)
             << " seconds.";

  localStore->clearCachesAndCompactAll();
  return 0;
}
