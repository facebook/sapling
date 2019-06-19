/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/Conv.h>
#include <folly/experimental/FunctionScheduler.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <thrift/lib/cpp2/server/ThriftServer.h>
#include <iostream>
#include <memory>
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/win/service/StartupLogger.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/io/IOBuf.h"

#ifndef _WIN32
#error This is a Windows only source file;
#endif
// DEFINE_bool(allowRoot, false, "Allow running eden directly as root");
// DEFINE_string(edenDir, "", "The path to the .eden directory");
// DEFINE_string(
//    etcEdenDir,
//    "/etc/eden",
//    "the directory holding all system configuration files");
// define_string(configpath, "", "the path of the ~/.edenrc config file");
// DEFINE_string(configPath, "", "The path of the ~/.edenrc config file");
// DEFINE_string(
//    logPath,
//    "if set, redirects stdout and stderr to the log file given.");

using namespace facebook::eden;
using namespace std;
using namespace folly;

// Set the default log level for all eden logs to DBG2
// Also change the "default" log handler (which logs to stderr) to log
// messages asynchronously rather than blocking in the logging thread.
FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

void debugSetLogLevel(std::string category, std::string level) {
  auto& db = folly::LoggerDB::get();
  db.getCategoryOrNull(category);
  folly::Logger(category).getCategory()->setLevel(
      folly::stringToLogLevel(level), true);
}

constexpr folly::StringPiece kDefaultUserConfigFile{".edenrc"};
constexpr folly::StringPiece kEdenfsConfigFile{"edenfs.rc"};

void startServer() {
  UserInfo identity;
  auto privHelper = make_unique<PrivHelper>();

  AbsolutePath userConfigPath =
      identity.getHomeDirectory() + PathComponentPiece{kDefaultUserConfigFile};
  AbsolutePath systemConfigDir =
      facebook::eden::realpath("c:\\eden\\etcedendir");
  const auto systemConfigPath =
      systemConfigDir + PathComponentPiece{kEdenfsConfigFile};

  auto edenConfig = std::make_unique<EdenConfig>(
      identity.getUsername(),
      identity.getUid(),
      identity.getHomeDirectory(),
      userConfigPath,
      systemConfigDir,
      systemConfigPath);

  auto prepareFuture = folly::Future<folly::Unit>::makeEmpty();
  auto startupLogger = std::make_shared<StartupLogger>();

  std::optional<EdenServer> server;
  try {
    server.emplace(
        std::move(identity), std::move(privHelper), std::move(edenConfig));
    prepareFuture = server->prepare(startupLogger);

    // startupLogger->log("Starting Eden");
  } catch (const std::exception& ex) {
    cout << "Error: failed to start Eden : " << folly::exceptionStr(ex) << endl;
    // startupLogger->exitUnsuccessfully(
    //    EX_SOFTWARE, "error starting edenfs: ", folly::exceptionStr(ex));
  }

  server->getServer()->serve();
  server->performCleanup();
}

int __cdecl main(int argc, char** argv) {
  XLOG(INFO) << "Eden Windows - started";

  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);
  // debugSetLogLevel("eden", "DBG");
  // debugSetLogLevel(".", "DBG");

  startServer();
  XLOG(INFO) << "Eden Windows - Stopped";
  return 0;
};
