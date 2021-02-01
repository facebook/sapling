/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/SocketAddress.h>
#include <folly/init/Init.h>
#include <folly/logging/Init.h>
#include <folly/logging/xlog.h>
#include "eden/fs/nfs/portmap/PortmapClient.h"

using namespace facebook::eden;

FOLLY_INIT_LOGGING_CONFIG("eden=INFO");

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  PortmapClient client;

  auto port =
      client.getPort(PortmapMapping{100003, 3, PortmapMapping::kProtoTcp, 0});

  XLOG(INFO) << "Got port: " << port;

  // Try to set a bogus port for NFS.
  // This will fail if there is already an NFS daemon running
  XLOG(INFO) << "Set mapping: "
             << client.setMapping(
                    PortmapMapping{100003, 3, PortmapMapping::kProtoTcp, 123});

  // Read back the current port
  auto newPort =
      client.getPort(PortmapMapping{100003, 3, PortmapMapping::kProtoTcp, 0});

  XLOG(INFO) << "Got new port: " << newPort;

  return 0;
}
