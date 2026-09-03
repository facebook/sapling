/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <cstring>
#include <string>

#ifdef __linux__
#include <sys/prctl.h>

#include <cerrno>

#include <folly/String.h>
#endif // __linux__

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/init/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include "eden/common/utils/UserInfo.h"
#include "eden/fs/privhelper/PinScan.h"
#include "eden/fs/privhelper/PrivHelperFlags.h"
#include "eden/fs/privhelper/PrivHelperRollback.h"
#include "eden/fs/privhelper/PrivHelperServer.h"

using namespace facebook::eden;

namespace {
struct PrivHelperOwner {
  uid_t uid;
  gid_t gid;
};

PrivHelperOwner resolvePrivHelperOwner(
    uid_t realUid,
    gid_t realGid,
    uid_t cliUid,
    gid_t cliGid) {
  if (realUid == 0) {
    return {cliUid, cliGid};
  }

  // A non-root real uid means argv is controlled by the caller, including the
  // installed setuid-root path. Only real-root launches may nominate a
  // different Eden owner, such as the sudo/dev flow using SUDO_UID.
  const auto hardeningDisabled = disablePrivHelperHardening();
  const auto ownerMismatch = cliUid != realUid || cliGid != realGid;
  if (ownerMismatch) {
    auto reason = std::string{"using real uid/gid"};
    if (hardeningDisabled) {
      reason = "honoring CLI values because `" +
          std::string{kDisablePrivHelperHardeningPath} + "` is present";
    }
    XLOGF(
        WARNING,
        "CLI-provided privhelper uid/gid {}/{} do not match real uid/gid {}/{}; {}",
        cliUid,
        cliGid,
        realUid,
        realGid,
        reason);
  } else if (hardeningDisabled) {
    XLOGF(
        WARNING,
        "Using CLI-provided privhelper uid/gid because `{}` is present",
        kDisablePrivHelperHardeningPath);
  }

  return hardeningDisabled ? PrivHelperOwner{cliUid, cliGid}
                           : PrivHelperOwner{realUid, realGid};
}
} // namespace

DEFINE_int32(
    privhelper_uid,
    facebook::eden::UserInfo::kDefaultNobodyUid,
    "The uid of the owner of this eden instance");

DEFINE_int32(
    privhelper_gid,
    facebook::eden::UserInfo::kDefaultNobodyGid,
    "The gid of the owner of this eden instance");

int main(int argc, char** argv) {
#ifdef __linux__
  // One-shot mode used by EdenFS pressure GC to discover pinned directories.
  // Handled before folly::Init so no flag or environment parsing happens on
  // this path: as a mode of a setuid binary it is invocable by any local
  // user, so it takes no input and reports only pins on the caller's own
  // mounts (see runScanPinsMode).
  //
  // The flag spelling matters: privhelper binaries that predate this mode
  // reject an unknown --flag with a clean exit(1) from gflags, whereas a
  // bare positional argument would fall through into server startup and
  // abort on the missing --privhelper_fd.
  if (argc == 2 && strcmp(argv[1], "--scan-pins") == 0) {
    return facebook::eden::runScanPinsMode();
  }
#endif

  const folly::Init init(&argc, &argv);

  auto loggingConfig = folly::parseLogConfig(
      "WARN:default, eden=DBG2; default:stream=stderr,async=false");
  folly::LoggerDB::get().updateConfig(loggingConfig);

#ifdef __linux__
  // The kernel clears the dumpable flag for setuid executions, so without
  // this privhelper crashes produce no core and never reach coredumper.
  if (prctl(PR_SET_DUMPABLE, 1, 0, 0, 0) != 0) {
    XLOGF(
        WARNING,
        "failed to mark privhelper dumpable: {}",
        folly::errnoStr(errno));
  }
#endif // __linux__

  // Escape the process group of whatever launched EdenFS, so that
  // process-group-wide cleanup (e.g. by agent command runners that
  // launched `eden restart`) cannot SIGKILL the privhelper out from under
  // a running daemon. See detachFromParentProcessGroup() for details.
  detachFromParentProcessGroup();

  PrivHelperServer server;
  try {
    // Redirect stdin
    folly::File devNullIn("/dev/null", O_RDONLY);
    auto retcode = folly::dup2NoInt(devNullIn.fd(), STDIN_FILENO);
    folly::checkUnixError(retcode, "failed to redirect stdin");

    folly::File serverConn(FLAGS_privhelper_fd, true);

    const auto realUid = getuid();
    const auto realGid = getgid();
    const auto cliUid = static_cast<uid_t>(FLAGS_privhelper_uid);
    const auto cliGid = static_cast<gid_t>(FLAGS_privhelper_gid);
    const auto owner = resolvePrivHelperOwner(realUid, realGid, cliUid, cliGid);

    server.init(std::move(serverConn), owner.uid, owner.gid);
    server.run();
    return 0;
  } catch (const std::exception& ex) {
    XLOGF(ERR, "error inside mount helper: {}", folly::exceptionStr(ex));
  } catch (...) {
    XLOG(ERR, "invalid type thrown inside mount helper");
  }

  return 1;
}
