/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <signal.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <sysexits.h>
#include <cstdio>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/init/Init.h>
#include <folly/logging/xlog.h>

#include "eden/fs/config/EdenConfig.h"
#include "eden/fs/monitor/EdenMonitor.h"
#include "eden/fs/service/EdenInit.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/UserInfo.h"

using std::string;
using namespace facebook::eden;

namespace {

FOLLY_NODISCARD folly::File openLockFile(AbsolutePathPiece edenDir) {
  ensureDirectoryExists(edenDir);
  auto lockPath = edenDir + "monitor.lock"_pc;
  auto lockFile =
      folly::File(lockPath.value(), O_RDWR | O_CREAT | O_CLOEXEC, 0644);
  if (!lockFile.try_lock()) {
    string existingPid;
    folly::readFile(lockFile.fd(), existingPid);
    throw_<std::runtime_error>(
        "another instance of the EdenFS monitor already "
        "appears to be running: pid ",
        existingPid);
  }

  // We acquired the lock.  Write our process ID to the lock file.
  auto pidString = folly::to<string>(getpid());
  ftruncate(lockFile.fd(), 0);
  auto writeResult =
      folly::writeFull(lockFile.fd(), pidString.data(), pidString.size());
  folly::checkUnixError(writeResult, "error writing process ID to lock file");
  return lockFile;
}

std::string findSelfExe() {
  // The maximum symlink limit is filesystem dependent, but many common Linux
  // filesystems have a limit of 4096.
  constexpr size_t pathMax = 4096;
  std::array<char, pathMax> buf;
  auto result = readlink("/proc/self/exe", buf.data(), buf.size());
  folly::checkUnixError(result, "failed to read /proc/self/exe");
  return std::string(buf.data(), static_cast<size_t>(result));
}

pid_t childPid;

void forwardSignal(int signum) {
  kill(childPid, signum);
}

// If we were started attached to a controlling terminal, explicitly fork
// and run the main monitor process in its own process group.
//
// This is helpful during development to ensure that the edenfs daemon won't
// be sent SIGINT twice if the developer hits Ctrl-C in their terminal.
// Hitting Ctrl-C in a terminal sends the signal to the entire process group.
// Since the monitor explicitly forward signals to its children edenfs processes
// we don't want them to receive both the signal that the monitor explicitly
// forwards as well as a signal to the terminal process group.  Running in a
// separate process group avoids this.
void newProcessGroup() {
  childPid = fork();
  folly::checkUnixError(childPid, "failed to fork");
  if (childPid == 0) {
    // Child process.
    auto rc = setsid();
    if (rc == -1) {
      XLOG(ERR) << "setsid() failed: " << folly::errnoStr(errno);
      // continue anyway
    }
    return;
  }

  // Forward any SIGTERM and SIGINT signals we receive to our child.
  struct sigaction act {};
  act.sa_handler = forwardSignal;
  auto rc = sigaction(SIGINT, &act, nullptr);
  folly::checkUnixError(rc, "failed to install SIGINT handler");
  rc = sigaction(SIGTERM, &act, nullptr);
  folly::checkUnixError(rc, "failed to install SIGTERM handler");

  // Wait for our child to exit.
  int status{};
  while (true) {
    auto waited = waitpid(childPid, &status, 0);
    if (waited == -1) {
      if (errno == EINTR) {
        continue;
      }
      XLOG(ERR) << "error waiting on forked child" << folly::errnoStr(errno);
      _exit(1);
    }
    break;
  }

  if (WIFEXITED(status)) {
    _exit(WEXITSTATUS(status));
  }
  // Our child exited with a signal.  If the signal was a terminal signal like
  // SIGINT/SIGTERM/SIGABRT/etc we could kill ourselves with kill() so that we
  // exit with the same signal.  However, just exiting with a specific status
  // code is simpler and probably good enough for now.
  _exit(127);
}

} // namespace

int main(int argc, char* argv[]) {
  std::vector<std::string> initialArgv;
  for (int n = 0; n < argc; ++n) {
    initialArgv.push_back(argv[n]);
  }
  folly::init(&argc, &argv);

  // If we happen to have been started attached to a controlling TTY,
  // fork once and run the monitor in its own process group, to avoid
  // double-delivering signals to our children EdenFS processes on Ctrl-C.
  if (isatty(STDIN_FILENO)) {
    newProcessGroup();
  }

  // Redirect stdin from /dev/null
  folly::File devNullIn("/dev/null", O_RDONLY);
  auto rc = folly::dup2NoInt(devNullIn.fd(), STDIN_FILENO);
  folly::checkUnixError(rc, "failed to redirect stdin");

  // Change directory to /
  rc = chdir("/");
  folly::checkUnixError(rc, "failed to chdir");

  // Find the location of our executable
  auto selfExe = findSelfExe();

  // Read the configuration to determine the EdenFS state directory
  auto identity = UserInfo::lookup();
  std::unique_ptr<EdenConfig> config;
  try {
    config = getEdenConfig(identity);
  } catch (const ArgumentError& ex) {
    fprintf(stderr, "%s\n", ex.what());
    return EX_SOFTWARE;
  }

  // Acquire a lock to ensure that there can only be once monitor process
  // running for a given EdenFS state directory.
  auto edenDir = config->edenDir.getValue();
  folly::File lockFile;
  try {
    lockFile = openLockFile(edenDir);
  } catch (const std::exception& ex) {
    fprintf(
        stderr, "failed to acquire the EdenFS monitor lock: %s\n", ex.what());
    return EX_SOFTWARE;
  }

  XLOG(INFO) << "Starting EdenFS monitor: pid " << getpid();
  EdenMonitor monitor(std::move(config), selfExe, initialArgv);
  monitor.run();
  return EX_OK;
}
