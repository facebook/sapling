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
#include "eden/fs/service/StartupLogger.h"
#include "eden/fs/telemetry/SessionInfo.h"
#include "eden/fs/win/utils/StringConv.h"
#include "folly/io/IOBuf.h"
#include "folly/portability/Windows.h"

#ifndef _WIN32
#error This is a Windows only source file;
#endif

using namespace facebook::eden;
using namespace std;
using namespace folly;

// TODO(puneetk): Logging on Windows doesn't work when the async is set. Fix the
// Async logging and enable the default logging below.

// Set the default log level for all eden logs to DBG2
// Also change the "default" log handler (which logs to stderr) to log
// messages asynchronously rather than blocking in the logging thread.
// FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG4");

// The --edenfs flag is defined to help make the flags consistent across Windows
// and non-Windows platforms.  On non-Windows platform this flag is required, as
// a check to help ensure that users do not accidentally invoke `edenfs` when
// they meant to run `edenfsctl`.
// It probably would be nice to eventually require this behavior on Windows too.
DEFINE_bool(
    edenfs,
    false,
    "This optional argument is currently ignored on Windows");

namespace {

constexpr StringPiece kEdenVersion = "edenwin";

} // namespace

int __cdecl main(int argc, char** argv) {
  XLOG(INFO) << "Eden Windows - starting";
  std::vector<std::string> originalCommandLine{argv, argv + argc};

  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);

  auto identity = UserInfo::lookup();
  auto privHelper = make_unique<PrivHelper>();

  std::unique_ptr<EdenConfig> edenConfig;
  try {
    edenConfig = getEdenConfig(identity);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return -1;
  }

  // Set some default glog settings, to be applied unless overridden on the
  // command line
  gflags::SetCommandLineOptionWithMode(
      "logtostderr", "1", gflags::SET_FLAGS_DEFAULT);
  gflags::SetCommandLineOptionWithMode(
      "minloglevel", "0", gflags::SET_FLAGS_DEFAULT);

  auto prepareFuture = folly::Future<folly::Unit>::makeEmpty();
  std::shared_ptr<StartupLogger> startupLogger;
  try {
    auto logPath = getLogPath(edenConfig->edenDir.getValue());
    startupLogger = daemonizeIfRequested(logPath);
  } catch (std::exception& ex) {
    // If the log redirection fails this error will show up on stderr. When Eden
    // is running in the background, this error will be lost. If the log file is
    // empty we should run the edenfs.exe on the console to get the error.
    fprintf(stderr, "%s\n", ex.what());
    return -1;
  }

  SessionInfo sessionInfo;
  sessionInfo.username = identity.getUsername();
  sessionInfo.hostname = getHostname();
  sessionInfo.os = getOperatingSystemName();
  sessionInfo.osVersion = getOperatingSystemVersion();
  sessionInfo.edenVersion = kEdenVersion.str();

  std::optional<EdenServer> server;
  try {
    server.emplace(
        std::move(originalCommandLine),
        std::move(identity),
        std::move(sessionInfo),
        std::move(privHelper),
        std::move(edenConfig));
    prepareFuture = server->prepare(startupLogger);
  } catch (const std::exception& ex) {
    fprintf(stderr, "Error: failed to start EdenFS: %s\n", ex.what());
    return -1;
  }

  try {
    server->getServer()->serve();
    server->performCleanup();
  } catch (const std::exception& ex) {
    fprintf(stderr, "Error while running EdenFS: %s\n", ex.what());
    return -1;
  }

  XLOG(INFO) << "Eden Windows - exiting";
  return 0;
};
