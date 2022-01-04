/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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

FOLLY_INIT_LOGGING_CONFIG("eden=DBG9");

int main(int argc, char** argv) {
  folly::init(&argc, &argv);

  PortmapClient client;

  auto addr = client.getAddr(PortmapMapping{100003, 3, "", "", ""});

  XLOG(INFO) << "Got addr: " << addr;

  // Try to set a bogus address for NFS.
  // This will fail if there is already an NFS daemon running
  XLOG(INFO) << "Set mapping: "
             << client.setMapping(
                    PortmapMapping{100003, 3, "tcp6", "::123", "edenfs"});

  // Read back the current address
  auto newAddr = client.getAddr(PortmapMapping{100003, 3, "", "", ""});

  XLOG(INFO) << "Got new addr: " << newAddr;

  return 0;
}
