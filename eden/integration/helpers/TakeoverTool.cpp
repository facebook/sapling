/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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

DEFINE_string(edenDir, "", "The path to the .eden directory");

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

  auto data = facebook::eden::takeoverMounts(takeoverSocketPath);
  for (const auto& mount : data.mountPoints) {
    XLOG(INFO) << "mount " << mount.mountPath << ": fd=" << mount.fuseFD.fd();
    for (const auto& bindMount : mount.bindMounts) {
      XLOG(INFO) << "  bind mount " << bindMount;
    }
  }
  return EX_OK;
}
