/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <sysexits.h>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include <gflags/gflags.h>
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/utils/FsChannelTypes.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");
/**
 * Versions 3 and 4 are the only valid versions to send here. Even if
 * a different version is specified, we still log version 3/4 message
 * contents.
 */
DEFINE_int32(takeoverVersion, 0, "The takeover version number to send");

DEFINE_bool(
    shouldPing,
    true,
    "This is used by integration tests to avoid sending a ping");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2");

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
  folly::init(&argc, &argv);

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }

  auto edenDir = facebook::eden::canonicalPath(FLAGS_edenDir);
  auto takeoverSocketPath = edenDir + "takeover"_pc;

  facebook::eden::TakeoverData data;
  if (FLAGS_takeoverVersion == 0) {
    data = facebook::eden::takeoverMounts(takeoverSocketPath, FLAGS_shouldPing);
  } else {
    auto takeoverVersion = std::set<int32_t>{FLAGS_takeoverVersion};
    data = facebook::eden::takeoverMounts(
        takeoverSocketPath, FLAGS_shouldPing, takeoverVersion);
  }
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
    XLOG(INFO) << "mount " << mount.mountPath << ": fd=" << mountFD->fd();
    for (const auto& bindMount : mount.bindMounts) {
      XLOG(INFO) << "  bind mount " << bindMount;
    }
  }
  return EX_OK;
}
