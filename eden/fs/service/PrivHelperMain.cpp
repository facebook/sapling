/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include "eden/common/utils/UserInfo.h"
#include "eden/fs/privhelper/PrivHelperFlags.h"
#include "eden/fs/privhelper/PrivHelperServer.h"

using namespace facebook::eden;

DEFINE_int32(
    privhelper_uid,
    facebook::eden::UserInfo::kDefaultNobodyUid,
    "The uid of the owner of this eden instance");

DEFINE_int32(
    privhelper_gid,
    facebook::eden::UserInfo::kDefaultNobodyGid,
    "The gid of the owner of this eden instance");

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

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

    server.init(
        std::move(serverConn), FLAGS_privhelper_uid, FLAGS_privhelper_gid);
    server.run();
    return 0;
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error inside mount helper: " << folly::exceptionStr(ex);
  } catch (...) {
    XLOG(ERR) << "invalid type thrown inside mount helper";
  }

  return 1;
}
