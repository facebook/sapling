/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestServer.h"

#include <folly/portability/GFlags.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/store/BackingStore.h"
#include "eden/fs/telemetry/IActivityRecorder.h"
#include "eden/fs/telemetry/IHiveLogger.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/testharness/FakePrivHelper.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/UserInfo.h"

using std::make_shared;
using std::make_unique;
using std::unique_ptr;

namespace facebook::eden {

namespace {

class EmptyBackingStoreFactory : public BackingStoreFactory {
  std::shared_ptr<BackingStore> createBackingStore(
      BackingStoreType,
      const CreateParams&) override {
    throw std::logic_error("TestServer has no BackingStores by default");
  }
};

EmptyBackingStoreFactory gEmptyBackingStoreFactory;

} // namespace

TestServer::TestServer()
    : tmpDir_(makeTempDir()), server_(createServer(getTmpDir())) {
  auto prepareResult = server_->prepare(make_shared<ForegroundStartupLogger>());
  // We don't care about waiting for prepareResult: it just indicates when
  // preparation has fully completed, but the EdenServer can begin being used
  // immediately, before prepareResult completes.
  //
  // Maybe in the future it would be worth storing this future in a member
  // variable so our caller could extract if if they want to.  (It would allow
  // the caller to schedule additional work once the thrift server is fully up
  // and running, if the caller starts the thrift server.)
  (void)prepareResult;
}

TestServer::~TestServer() {}

AbsolutePath TestServer::getTmpDir() const {
  return canonicalPath(tmpDir_.path().string());
}

unique_ptr<EdenServer> TestServer::createServer(AbsolutePathPiece tmpDir) {
  auto edenDir = tmpDir + "eden"_pc;
  ensureDirectoryExists(edenDir);

  // Always use an in-memory local store during tests.
  // TODO: in the future we should build a better mechanism for controlling this
  // rather than having to update a command line flag.
  GFLAGS_NAMESPACE::SetCommandLineOptionWithMode(
      "local_storage_engine_unsafe",
      "memory",
      GFLAGS_NAMESPACE::SET_FLAG_IF_DEFAULT);

  auto userInfo = UserInfo::lookup();
  userInfo.setHomeDirectory(tmpDir + "home"_pc);
  auto config = make_shared<EdenConfig>(
      getUserConfigVariables(userInfo),
      userInfo.getHomeDirectory(),
      userInfo.getHomeDirectory() + ".edenrc"_pc,
      tmpDir + "etc"_pc,
      tmpDir + "etc/edenfs.rc"_relpath);
  auto privHelper = make_unique<FakePrivHelper>();
  config->edenDir.setValue(edenDir, ConfigSourceType::CommandLine);

  return make_unique<EdenServer>(
      std::vector<std::string>{"edenfs_unit_test"},
      userInfo,
      SessionInfo{},
      std::move(privHelper),
      config,
      [](std::shared_ptr<const EdenMount>) {
        return std::make_unique<NullActivityRecorder>();
      },
      &gEmptyBackingStoreFactory,
      make_shared<NullHiveLogger>(),
      "test server");
}

} // namespace facebook::eden
