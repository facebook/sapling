/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <string>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/init/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Unistd.h>
#include "eden/common/utils/UserInfo.h"
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
  const folly::Init init(&argc, &argv);

  auto loggingConfig = folly::parseLogConfig(
      "WARN:default, eden=DBG2; default:stream=stderr,async=false");
  folly::LoggerDB::get().updateConfig(loggingConfig);

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
