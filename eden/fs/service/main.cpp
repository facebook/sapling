/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include <folly/Conv.h>
#include <folly/init/Init.h>
#include <gflags/gflags.h>
#include <pwd.h>
#include <stdlib.h>
#include <sysexits.h>
#include "EdenServer.h"
#include "eden/fuse/privhelper/PrivHelper.h"

DEFINE_bool(allowRoot, false, "Allow running eden directly as root");
DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(
    systemConfigDir,
    "/etc/eden/config.d",
    "The directory holding all system configuration files");
DEFINE_string(configPath, "", "The path of the ~/.edenrc config file");
DEFINE_string(rocksPath, "", "The path to the local RocksDB store");

DEFINE_int32(
    fuseThreadStack,
    1 * 1024 * 1024,
    "thread stack size for fuse dispatcher threads");

using namespace facebook::eden::fusell;
using namespace facebook::eden;

uid_t determineUid() {
  // First check the real UID.  If it is non-root, use that.
  // This happens if our binary is setuid root and invoked by a non-root user.
  auto uid = getuid();
  if (uid != 0) {
    return uid;
  }

  // If the real UID is 0, check fo see if we are running under sudo.
  auto sudoUid = getenv("SUDO_UID");
  if (sudoUid != nullptr) {
    try {
      return folly::to<uid_t>(sudoUid);
    } catch (const std::range_error& ex) {
      // Bad value in the SUDO_UID environment variable.
      // Ignore it and fall through
    }
  }

  return uid;
}

gid_t determineGid() {
  // Check the real GID first.
  auto gid = getgid();
  if (gid != 0) {
    return gid;
  }

  auto sudoGid = getenv("SUDO_GID");
  if (sudoGid != nullptr) {
    try {
      return folly::to<gid_t>(sudoGid);
    } catch (const std::range_error& ex) {
      // Bad value in the SUDO_GID environment variable.
      // Ignore it and fall through
    }
  }

  return gid;
}

int main(int argc, char **argv) {
  // Make sure to run this before any flag values are read.
  folly::init(&argc, &argv);

  // Determine the desired user and group ID.
  if (geteuid() != 0) {
    fprintf(stderr, "error: edenfs must be started as root\n");
    return EX_NOPERM;
  }
  uid_t uid = determineUid();
  gid_t gid = determineGid();
  if (uid == 0 && !FLAGS_allowRoot) {
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
  google::SetCommandLineOptionWithMode(
      "logtostderr", "1", google::SET_FLAGS_DEFAULT);
  google::SetCommandLineOptionWithMode(
      "minloglevel", "0", google::SET_FLAGS_DEFAULT);

  // Fork the privhelper process, then drop privileges in the main process.
  // This should be done as early as possible, so that everything else we do
  // runs only with normal user privileges.
  //
  // (It might be better to do this even before calling folly::init() and
  // parsing command line arguments.  The downside would be that we then
  // shouldn't really use glog in the privhelper process, since it won't have
  // been set up and configured based on the command line flags.)
  fusell::startPrivHelper(uid, gid);
  fusell::dropPrivileges();
  LOG(INFO) << "Starting edenfs.  UID=" << uid << ", GID=" << gid
            << ", PID=" << getpid();

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }
  auto edenDir = canonicalPath(FLAGS_edenDir);
  auto systemConfigDir = FLAGS_systemConfigDir.empty()
      ? AbsolutePath{"/etc/eden/config.d"}
      : canonicalPath(FLAGS_systemConfigDir);
  auto rocksPath = FLAGS_rocksPath.empty()
      ? edenDir + RelativePathPiece{"storage/rocks-db"}
      : canonicalPath(FLAGS_rocksPath);

  std::string configPathStr = FLAGS_configPath;
  if (configPathStr.empty()) {
    auto homeDir = getenv("HOME");
    if (homeDir) {
      configPathStr = homeDir;
    } else {
      struct passwd pwd;
      struct passwd* result;
      char buf[1024];
      if (getpwuid_r(getuid(), &pwd, buf, sizeof(buf), &result) == 0) {
        if (result != nullptr) {
          configPathStr = pwd.pw_dir;
        }
      }
    }
    if (configPathStr.empty()) {
      fprintf(
          stderr,
          "error: the --configPath argument was not specified and no "
          "$HOME directory could be found for this user\n");
      return EX_USAGE;
    }
    configPathStr.append("/.edenrc");
  }
  auto configPath = canonicalPath(configPathStr);

  // Set the FUSE_THREAD_STACK environment variable.
  // Do this early on before we spawn any other threads, since setenv()
  // is not thread-safe.
  setenv(
      "FUSE_THREAD_STACK",
      folly::to<std::string>(FLAGS_fuseThreadStack).c_str(),
      1);

  // Run the eden server
  EdenServer server(edenDir, systemConfigDir, configPath, rocksPath);
  server.run();

  LOG(INFO) << "edenfs performing orderly shutdown";

  return EX_OK;
}
