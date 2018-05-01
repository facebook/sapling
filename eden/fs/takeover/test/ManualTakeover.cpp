/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <sysexits.h>

#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include "eden/fs/takeover/TakeoverClient.h"
#include "eden/fs/takeover/TakeoverData.h"

DEFINE_string(edenDir, "", "The path to the .eden directory");
DEFINE_string(logging, "", "Logging configuration");

FOLLY_INIT_LOGGING_CONFIG("eden=DBG2");

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
  folly::initLogging(FLAGS_logging);

  if (FLAGS_edenDir.empty()) {
    fprintf(stderr, "error: the --edenDir argument is required\n");
    return EX_USAGE;
  }

  auto edenDir = facebook::eden::canonicalPath(FLAGS_edenDir);
  auto takeoverSocketPath =
      edenDir + facebook::eden::PathComponentPiece{"takeover"};

  auto data = facebook::eden::takeoverMounts(takeoverSocketPath);
  for (const auto& mount : data.mountPoints) {
    XLOG(INFO) << "mount " << mount.mountPath << ": fd=" << mount.fuseFD.fd();
    for (const auto& bindMount : mount.bindMounts) {
      XLOG(INFO) << "  bind mount " << bindMount;
    }
  }
  return EX_OK;
}
