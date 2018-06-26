/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/init/Init.h>
#include <folly/io/async/AsyncSignalHandler.h>
#include <folly/logging/Init.h>
#include <gflags/gflags.h>
#include <signal.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <array>

#include "eden/fs/fuse/privhelper/UserInfo.h"
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/service/gen-cpp2/StreamingEdenService.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;
using apache::thrift::ThriftServer;
using facebook::fb303::cpp2::fb_status;
using folly::EventBase;
using std::make_shared;

DEFINE_bool(allowRoot, false, "Allow running eden directly as root");
DEFINE_bool(foreground, false, "Run edenfs in the foreground");
DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(
    etcEdenDir,
    "/etc/eden",
    "The directory holding all system configuration files");
DEFINE_string(configPath, "", "The path of the ~/.edenrc config file");
DEFINE_string(
    logPath,
    "",
    "If set, redirects stdout and stderr to the log file given.");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2");

namespace {
class FakeEdenServiceHandler : virtual public StreamingEdenServiceSvIf {
 public:
  FakeEdenServiceHandler() {}

  fb_status getStatus() override {
    return fb_status::ALIVE;
  }

  int64_t getPid() override {
    return getpid();
  }

  void listMounts(std::vector<MountInfo>& /* results */) override {
    return;
  }

  void shutdown() override {
    printf("received shutdown() thrift request\n");
  }

  void initiateShutdown(std::unique_ptr<std::string> reason) override {
    printf(
        "received initiateShutdown() thrift requested: %s\n", reason->c_str());
  }
};

class SignalHandler : public folly::AsyncSignalHandler {
 public:
  explicit SignalHandler(EventBase* eventBase) : AsyncSignalHandler(eventBase) {
    registerSignalHandler(SIGINT);
    registerSignalHandler(SIGTERM);
  }

  void signalReceived(int sig) noexcept override {
    // We just print a message when we receive a signal,
    // but ignore it otherwise
    switch (sig) {
      case SIGINT:
        printf("received SIGINT\n");
        break;
      case SIGTERM:
        printf("received SIGTERM\n");
        break;
      default:
        printf("received signal %d\n", sig);
        break;
    }
  }
};

bool acquireLock(AbsolutePathPiece edenDir) {
  const auto lockPath = edenDir + "lock"_pc;
  auto lockFile = folly::File(lockPath.value(), O_WRONLY | O_CREAT);
  if (!lockFile.try_lock()) {
    return false;
  }

  // Write the PID (with a newline) to the lockfile.
  folly::ftruncateNoInt(lockFile.fd(), /* len */ 0);
  const auto pidContents = folly::to<std::string>(getpid(), "\n");
  folly::writeNoInt(lockFile.fd(), pidContents.data(), pidContents.size());

  // Intentionally leak the lock FD so we hold onto it until we exit.
  lockFile.release();
  return true;
}
} // namespace

int main(int argc, char** argv) {
  // Drop privileges
  auto identity = UserInfo::lookup();
  identity.dropPrivileges();

  auto init = folly::Init(&argc, &argv);

  StartupLogger startupLogger;
  if (!FLAGS_foreground) {
    startupLogger.daemonize(FLAGS_logPath);
  }

  if (FLAGS_edenDir.empty()) {
    startupLogger.exitUnsuccessfully(1, "the --edenDir flag is required");
  }
  auto edenDir = facebook::eden::canonicalPath(FLAGS_edenDir);

  // Acquire the lock file
  if (!acquireLock(edenDir)) {
    startupLogger.exitUnsuccessfully(1, "Failed to acquire lock file");
  }

  // Get the path to the thrift socket.
  auto thriftSocketPath = edenDir + "socket"_pc;
  folly::SocketAddress thriftAddress;
  thriftAddress.setFromPath(thriftSocketPath.stringPiece());

  // Make sure no socket already exists at this path
  int rc = unlink(thriftSocketPath.value().c_str());
  if (rc != 0 && errno != ENOENT) {
    int errnum = errno;
    startupLogger.exitUnsuccessfully(
        1,
        "failed to remove eden socket at ",
        thriftSocketPath,
        ": ",
        folly::errnoStr(errnum));
  }

  // Create the ThriftServer object
  auto handler = make_shared<FakeEdenServiceHandler>();
  ThriftServer server;
  server.setInterface(handler);
  server.setAddress(thriftAddress);

  // Set up a signal handler to ignore SIGINT and SIGTERM
  // This lets our integration tests exercise the case where edenfs does not
  // shut down on its own.
  SignalHandler signalHandler(server.getEventBaseManager()->getEventBase());

  // Run the thrift server
  server.setup();
  startupLogger.success();
  folly::EventBaseManager::get()->getEventBase()->loopForever();

  return 0;
}
