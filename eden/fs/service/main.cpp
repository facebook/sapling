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
#include <folly/experimental/logging/Init.h>
#include <folly/experimental/logging/xlog.h>
#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <pwd.h>
#include <stdlib.h>
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
DEFINE_string(rocksPath, "", "The path to the local RocksDB store");

// The logging configuration parameter.  We default to INFO for everything in
// eden, and WARNING for all other categories.
DEFINE_string(logging, ".=WARNING,eden=INFO", "Logging configuration");

DEFINE_int32(
    fuseThreadStack,
    1 * 1024 * 1024,
    "thread stack size for fuse dispatcher threads");

using namespace facebook::eden::fusell;
using namespace facebook::eden;

namespace facebook {
namespace eden {
void runServer(const EdenServer& server);
}
}

int main(int argc, char **argv) {
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
  fusell::startPrivHelper(identity.getUid(), identity.getGid());
  fusell::dropPrivileges();

  folly::initLoggingGlogStyle(FLAGS_logging, folly::LogLevel::WARNING);

  XLOG(INFO) << "Starting edenfs.  UID=" << identity.getUid()
             << ", GID=" << identity.getGid() << ", PID=" << getpid();

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }
  auto edenDir = canonicalPath(FLAGS_edenDir);
  auto etcEdenDir = canonicalPath(FLAGS_etcEdenDir);
  auto rocksPath = FLAGS_rocksPath.empty()
      ? edenDir + RelativePathPiece{"storage/rocks-db"}
      : canonicalPath(FLAGS_rocksPath);

  AbsolutePath configPath;
  std::string configPathStr = FLAGS_configPath;
  if (configPathStr.empty()) {
    configPath = identity.getHomeDirectory() + PathComponentPiece{".edenrc"};
  } else {
    configPath = canonicalPath(configPathStr);
  }

  // Set the FUSE_THREAD_STACK environment variable.
  // Do this early on before we spawn any other threads, since setenv()
  // is not thread-safe.
  setenv(
      "FUSE_THREAD_STACK",
      folly::to<std::string>(FLAGS_fuseThreadStack).c_str(),
      1);

  // Create the eden server
  EdenServer server(edenDir, etcEdenDir, configPath, rocksPath);

  // Start stats aggregation
  folly::FunctionScheduler functionScheduler;
  functionScheduler.addFunction(
      [&server] { server.getStats()->get()->aggregate(); },
      std::chrono::seconds(1));
  functionScheduler.setThreadName("stats_aggregator");
  functionScheduler.start();

  // Get the EdenServer ready, then run the thrift server.
  server.prepare();
  runServer(server);

  XLOG(INFO) << "edenfs performing orderly shutdown";
  functionScheduler.shutdown();

  // Clean up all the server mount points before shutting down the privhelper
  server.unmountAll();

  // Explicitly stop the privhelper process so we can verify that it
  // exits normally.
  auto privhelperExitCode = fusell::stopPrivHelper();
  if (privhelperExitCode != 0) {
    if (privhelperExitCode > 0) {
      XLOG(WARNING) << "privhelper process exited with unexpected code "
                    << privhelperExitCode;
    } else {
      XLOG(WARNING) << "privhelper process was killed by signal "
                    << privhelperExitCode;
    }
    return EX_SOFTWARE;
  }
  return EX_OK;
}
