/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/Conv.h>
#include <folly/experimental/FunctionScheduler.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include <pwd.h>
#include <sysexits.h>
#include "EdenServer.h"
#include "eden/fs/fuse/privhelper/PrivHelper.h"
#include "eden/fs/fuse/privhelper/UserInfo.h"

DEFINE_bool(allowRoot, false, "Allow running eden directly as root");
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

DEFINE_string(logging, "", "Logging configuration");

using namespace facebook::eden;

// Set the default log level for all eden logs to DBG2
// Also change the "default" log handler (which logs to stderr) to log
// messages asynchronously rather than blocking in the logging thread.
FOLLY_INIT_LOGGING_CONFIG("eden=DBG2; default:async=true");

int main(int argc, char** argv) {
  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);

  // Determine the desired user and group ID.
  if (geteuid() != 0) {
    fprintf(stderr, "error: edenfs must be started as root\n");
    return EX_NOPERM;
  }

  auto identity = UserInfo::lookup();
  if (identity.getUid() == 0 && !FLAGS_allowRoot) {
    fprintf(
        stderr,
        "error: you appear to be running eden as root, "
        "rather than using\n"
        "sudo or a setuid binary.  This is normally undesirable.\n"
        "Pass in the --allowRoot flag if you really mean to run "
        "eden as root.\n");
    return EX_USAGE;
  }

  // If logPath is set, redirect stdout and stderr.
  if (!FLAGS_logPath.empty()) {
    EffectiveUserScope effectiveUserScope(identity);
    folly::File logHandle(FLAGS_logPath, O_APPEND | O_CREAT | O_WRONLY, 0644);
    folly::checkUnixError(dup2(logHandle.fd(), STDOUT_FILENO));
    folly::checkUnixError(dup2(logHandle.fd(), STDERR_FILENO));
  }

  // Since we are a daemon, and we don't ever want to be in a situation
  // where we hold any open descriptors through a fuse mount that points
  // to ourselves (which can happen during takeover), we chdir to `/`
  // to avoid having our cwd reference ourselves if the user runs
  // `eden daemon --takeover` from within an eden mount
  folly::checkPosixError(chdir("/"), "failed to chdir(/)");

  // Set some default glog settings, to be applied unless overridden on the
  // command line
  gflags::SetCommandLineOptionWithMode(
      "logtostderr", "1", gflags::SET_FLAGS_DEFAULT);
  gflags::SetCommandLineOptionWithMode(
      "minloglevel", "0", gflags::SET_FLAGS_DEFAULT);

  // Fork the privhelper process, then drop privileges in the main process.
  // This should be done as early as possible, so that everything else we do
  // runs only with normal user privileges.
  //
  // (It might be better to do this even before calling folly::init() and
  // parsing command line arguments.  The downside would be that we then
  // shouldn't really use glog in the privhelper process, since it won't have
  // been set up and configured based on the command line flags.)
  auto privHelper = startPrivHelper(identity);
  identity.dropPrivileges();

  folly::initLogging(FLAGS_logging);

  XLOG(INFO) << "Starting edenfs.  UID=" << identity.getUid()
             << ", GID=" << identity.getGid() << ", PID=" << getpid();

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }
  // We require edenDir to already exist, so use realpath() to resolve it.
  const auto edenDir = facebook::eden::realpath(FLAGS_edenDir);

  // It's okay if the etcEdenDir and configPath don't exist, so use
  // normalizeBestEffort() to try resolving symlinks in these paths but don't
  // fail if they don't exist.
  const auto etcEdenDir = normalizeBestEffort(FLAGS_etcEdenDir);

  const std::string configPathStr = FLAGS_configPath;
  const AbsolutePath configPath = configPathStr.empty()
      ? identity.getHomeDirectory() + PathComponentPiece{".edenrc"}
      : normalizeBestEffort(configPathStr);

  EdenServer server(
      std::move(identity),
      std::move(privHelper),
      edenDir,
      etcEdenDir,
      configPath);
  server.run();
  XLOG(INFO) << "edenfs exiting successfully";
  return EX_OK;
}
