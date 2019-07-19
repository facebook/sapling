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
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/service/EdenServer.h"
#include "eden/fs/win/service/StartupLogger.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/io/IOBuf.h"

#ifndef _WIN32
#error This is a Windows only source file;
#endif

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

int __cdecl main(int argc, char** argv) {
  XLOG(INFO) << "Eden Windows - starting";
  std::vector<std::string> originalCommandLine{argv, argv + argc};

  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);

  UserInfo identity;
  auto privHelper = make_unique<PrivHelper>();

  std::unique_ptr<EdenConfig> edenConfig;
  try {
    edenConfig = getEdenConfig(identity);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return -1;
  }

  auto prepareFuture = folly::Future<folly::Unit>::makeEmpty();
  auto startupLogger = std::make_shared<StartupLogger>();

  std::optional<EdenServer> server;
  try {
    server.emplace(
        std::move(originalCommandLine),
        std::move(identity),
        std::move(privHelper),
        std::move(edenConfig));
    prepareFuture = server->prepare(startupLogger);

    // startupLogger->log("Starting Eden");
  } catch (const std::exception& ex) {
    fprintf(stderr, "Error: failed to start Eden: %s\n", ex.what());
    return -1;
    // startupLogger->exitUnsuccessfully(
    //    EX_SOFTWARE, "error starting edenfs: ", folly::exceptionStr(ex));
  }

  server->getServer()->serve();
  server->performCleanup();

  XLOG(INFO) << "Eden Windows - exiting";
  return 0;
};
