/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "WinService.h"
#include <winerror.h>

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
#include "eden/fs/win/utils/WinError.h"
#include "folly/io/IOBuf.h"

using namespace std;
using namespace folly;

// TODO(puneetk): Logging on Windows doesn't work when the async is set. Fix the
// Async logging and enable the default logging below.

// Set the default log level for all eden logs to DBG2
// Also change the "default" log handler (which logs to stderr) to log
// messages asynchronously rather than blocking in the logging thread.
// FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");
FOLLY_INIT_LOGGING_CONFIG("eden=DBG4");

namespace facebook {
namespace eden {
namespace {

#define NO_ERROR 0
#define SVCNAME L"Edenfs"
constexpr StringPiece kEdenVersion = "edenwin";

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
    throw facebook::eden::makeWin32ErrorExplicit(
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

  if (_dup2(fd, STDERR_FILENO) == -1) {
    throw std::runtime_error(
        folly::format("Dup failed to update stderr. errno: {}", errno).str());
  }

  if (_dup2(fd, STDOUT_FILENO) == -1) {
    throw std::runtime_error(
        folly::format("Dup failed to update stdout. errno: {}", errno).str());
  }

  SCOPE_EXIT {
    _close(fd);
  };
}

/**
 * We create WinService as a global variable, so we could use it in the static
 * functions callbacks from Windows SCM and our C++ class.
 */
WinService service;

const SERVICE_TABLE_ENTRY dispatchTable[] = {
    {(LPWSTR)SVCNAME, (LPSERVICE_MAIN_FUNCTION)WinService::main},
    {nullptr, nullptr}};
} // namespace
void WINAPI WinService::main(DWORD argc, LPSTR* argv) {
  service.serviceMain(argc, argv);
}

void WinService::create(int argc, char** argv) {
  auto identity = UserInfo::lookup();
  auto userHome = identity.getHomeDirectory().stringPiece().toString();
  std::string dotEden = userHome + "\\.eden";
  std::string logFile = dotEden + "\\edenstartup.log";

  if (!CreateDirectoryA(dotEden.c_str(), nullptr)) {
    DWORD error = GetLastError();
    if (error != ERROR_ALREADY_EXISTS) {
      // If it fails to create the .eden directory for some reason - it won't
      // fail here but changes the startup log path.
      logFile = userHome + "\\edenstartup.log";
    }
  }
  redirectLogOutput(logFile.c_str());
  XLOG(INFO) << "Starting Eden Service";

  // This call returns when the service has stopped.
  // The process should simply terminate when the call returns.
  if (!StartServiceCtrlDispatcher(dispatchTable)) {
    XLOG(ERR) << "Failed :" << GetLastError();
  }
  XLOG(INFO) << "Service Exited" << GetLastError();
}

int WinService::serviceMain(int argc, char** argv) {
  handle_ = RegisterServiceCtrlHandler(SVCNAME, ctrlHandler);
  if (!handle_) {
    fprintf(
        stderr,
        "RegisterServiceCtrlHandler failed. error %d \n",
        GetLastError());
    return -1;
  }

  status_.dwServiceType = SERVICE_USER_OWN_PROCESS;
  status_.dwServiceSpecificExitCode = 0;

  // Setting a 3000 millisecond estimated wait for the the start pending. We
  // should not need more than this. If this starts to timeout we could increase
  // the wait hint.
  reportStatus(SERVICE_START_PENDING, NO_ERROR, 3000);
  setup(argc, argv);
  reportStatus(SERVICE_RUNNING, NO_ERROR, 0);
  run();
  reportStatus(SERVICE_STOPPED, NO_ERROR, 0);

  XLOG(INFO) << "Eden Windows - exiting";
  return 0;
}

int WinService::setup(int argc, char** argv) {
  auto identity = UserInfo::lookup();
  auto privHelper = make_unique<PrivHelper>();
  std::vector<std::string> originalCommandLine{argv, argv + argc};

  std::unique_ptr<EdenConfig> edenConfig;
  try {
    edenConfig = getEdenConfig(identity);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return -1;
  }

  try {
    auto logPath = getLogPath(edenConfig->edenDir.getValue());
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
  auto startupLogger = std::make_shared<ForegroundStartupLogger>();

  SessionInfo sessionInfo;
  sessionInfo.username = identity.getUsername();
  sessionInfo.hostname = getHostname();
  sessionInfo.os = getOperatingSystemName();
  sessionInfo.osVersion = getOperatingSystemVersion();
  sessionInfo.edenVersion = kEdenVersion.str();

  try {
    server_.emplace(
        std::move(originalCommandLine),
        std::move(identity),
        std::move(sessionInfo),
        std::move(privHelper),
        std::move(edenConfig));
    prepareFuture = server_->prepare(startupLogger);
  } catch (const std::exception& ex) {
    fprintf(stderr, "Error: failed to start Eden: %s\n", ex.what());
    return -1;
  }

  return 0;
};

void WinService::run() {
  server_->getServer()->serve();
  server_->performCleanup();
}

VOID WinService::reportStatus(
    DWORD currentState,
    DWORD exitCode,
    DWORD waitHint) {
  status_.dwCurrentState = currentState;
  status_.dwWin32ExitCode = exitCode;
  status_.dwWaitHint = waitHint;

  if (currentState == SERVICE_START_PENDING)
    status_.dwControlsAccepted = 0;
  else
    status_.dwControlsAccepted = SERVICE_ACCEPT_STOP;

  if ((currentState == SERVICE_RUNNING) || (currentState == SERVICE_STOPPED))
    status_.dwCheckPoint = 0;
  else
    status_.dwCheckPoint = dwCheckPoint_++;

  // Report the status of the service to the SCM.
  SetServiceStatus(handle_, &status_);
}

void WinService::stop() {
  if (server_.has_value()) {
    server_.value().stop();
  }
}

void WINAPI WinService::ctrlHandler(DWORD dwCtrl) {
  switch (dwCtrl) {
    case SERVICE_CONTROL_STOP:
      service.reportStatus(SERVICE_STOP_PENDING, NO_ERROR, 0);
      service.stop();
      return;

    default:
      break;
  }
}

} // namespace eden
} // namespace facebook
