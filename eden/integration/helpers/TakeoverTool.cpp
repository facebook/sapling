/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <sysexits.h>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GFlags.h>
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/utils/FsChannelTypes.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");

DEFINE_bool(
    shouldPing,
    true,
    "This is used by integration tests to avoid sending a ping");

DEFINE_bool(
    shouldThrowDuringTakeover,
    false,
    "This can be used by tests to initiate an error in the TakeoverClient");

using namespace facebook::eden::path_literals;

/*
 * This is a small tool for manually exercising the edenfs takover code.
 *
 * This connects to an existing edenfs daemon and requests to take over its
 * mount points.  It prints out the mount points received and then exits.
 * Note that it does not unmount them before exiting, so the mount points will
 * need to be manually unmounted afterwards.
 */
int main(int argc, char* argv[]) {
  const folly::Init init(&argc, &argv);

  auto loggingConfig = folly::parseLogConfig("eden=DBG2");
  folly::LoggerDB::get().updateConfig(loggingConfig);

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }

  auto edenDir = facebook::eden::canonicalPath(FLAGS_edenDir);
  auto takeoverSocketPath = edenDir + "takeover"_pc;

  const std::chrono::seconds& takeoverReceiveTimeout =
      std::chrono::seconds(150);
  auto data = facebook::eden::takeoverMounts(
      takeoverSocketPath,
      takeoverReceiveTimeout,
      /*force=*/false,
      /*onlyIfIdle=*/false,
      FLAGS_shouldThrowDuringTakeover,
      FLAGS_shouldPing);
  for (const auto& mount : data.mountPoints) {
    const folly::File* mountFD = nullptr;
    if (auto fuseChannelData =
            std::get_if<facebook::eden::FuseChannelData>(&mount.channelInfo)) {
      mountFD = &fuseChannelData->fd;
    } else {
      auto& nfsChannelData =
          std::get<facebook::eden::NfsChannelData>(mount.channelInfo);
      mountFD = &nfsChannelData.nfsdSocketFd;
    }
    XLOGF(INFO, "mount {}: fd={}", mount.mountPath, mountFD->fd());
  }
  return EX_OK;
}
