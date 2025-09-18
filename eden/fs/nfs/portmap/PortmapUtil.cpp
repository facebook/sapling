/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/SocketAddress.h>
#include <folly/init/Init.h>
#include <folly/logging/LogConfigParser.h>
#include <folly/logging/xlog.h>
#include "eden/fs/nfs/portmap/PortmapClient.h"

using namespace facebook::eden;

int main(int argc, char** argv) {
  const folly::Init init(&argc, &argv);

  auto loggingConfig = folly::parseLogConfig("eden=DBG9");
  folly::LoggerDB::get().updateConfig(loggingConfig);

  PortmapClient client;

  auto addr = client.getAddr(PortmapMapping4{100003, 3, "", "", ""});

  XLOGF(INFO, "Got addr: {}", addr);

  // Try to set a bogus address for NFS.
  // This will fail if there is already an NFS daemon running
  XLOGF(
      INFO,
      "Set mapping: {}",
      client.setMapping(PortmapMapping4{100003, 3, "tcp6", "::123", "edenfs"}));

  // Read back the current address
  auto newAddr = client.getAddr(PortmapMapping4{100003, 3, "", "", ""});

  XLOGF(INFO, "Got new addr: {}", newAddr);

  return 0;
}
