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
#include "folly/portability/Windows.h"

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

void redirectLogOutput(const char* logPath) {
  HANDLE newHandle = CreateFileA(
      logPath,
      FILE_APPEND_DATA,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      OPEN_ALWAYS,
      FILE_ATTRIBUTE_NORMAL,
      nullptr);

  if (newHandle == INVALID_HANDLE_VALUE) {
    throw makeWin32ErrorExplicit(
        GetLastError(),
        folly::sformat("Unable to open the log file {}\n", logPath));
  }

  // Don't close the previous handles here, it will be closed as part of _dup2
  // call.

  SetStdHandle(STD_OUTPUT_HANDLE, newHandle);
  SetStdHandle(STD_ERROR_HANDLE, newHandle);

  int fd = _open_osfhandle(reinterpret_cast<intptr_t>(newHandle), _O_APPEND);

  if (fd == -1) {
    throw std::runtime_error(
        "_open_osfhandle() returned -1 while opening logfile");
  }

  if (_dup2(fd, _fileno(stderr)) == -1) {
    throw std::runtime_error(
        folly::format("Dup failed to update stderr. errno: {}", errno).str());
  }

  if (_dup2(fd, _fileno(stdout)) == -1) {
    throw std::runtime_error(
        folly::format("Dup failed to update stdout. errno: {}", errno).str());
  }

  SCOPE_EXIT {
    _close(fd);
  };
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

  try {
    auto logPath = getLogPath(edenConfig->getEdenDir());
    if (!logPath.empty()) {
      redirectLogOutput(logPath.c_str());
    }
  } catch (std::exception& ex) {
    // If the log redirection fails this error will show up on stderr. When Eden
    // is running in the background, this error will be lost. If the log file is
    // empty we should run the edenfs.exe on the console to get the error.
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
