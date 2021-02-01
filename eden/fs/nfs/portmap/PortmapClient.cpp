/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/portmap/PortmapClient.h"
#include <common/network/NetworkUtil.h>
#include <folly/Exception.h>
#include <folly/SocketAddress.h>
#include <folly/String.h>
#include <folly/logging/xlog.h>

using folly::IOBuf;
using folly::IOBufQueue;
using folly::SocketAddress;

namespace facebook::eden {

namespace {
constexpr uint32_t kPortmapPortNumber = 111;
constexpr uint32_t kPortmapProgNumber = 100000;
/*
 * Ideally we should use version 3 and 4, as that appears to have better
 * support for ipv6. For now, and since the goal is to only support a loopback
 * NFS, let's use version 2 and ipv4.
 */
constexpr uint32_t kPortmapVersionNumber = 2;
constexpr uint32_t kPortmapSet = 1;
constexpr uint32_t kPortmapUnSet = 2;
constexpr uint32_t kPortmapGetPort = 3;
} // namespace

EDEN_XDR_SERDE_IMPL(PortmapMapping, prog, vers, prot, port);

PortmapClient::PortmapClient()
    : client_(SocketAddress(
          network::NetworkUtil::getLocalIPv4(),
          kPortmapPortNumber)) {
#ifdef __APPLE__
  {
    // Connect to the portmap "tickler" socket.
    // This causes launchd to spawn `rpcbind` and bring up the portmap service.
    auto addr = SocketAddress::makeFromPath("/var/run/portmap.socket");
    sockaddr_storage stg;
    auto len = addr.getAddress(&stg);
    tickler_ = folly::netops::socket(addr.getFamily(), SOCK_STREAM, 0);
    folly::checkUnixError(
        folly::netops::connect(tickler_, (sockaddr*)&stg, len),
        "connect to ",
        addr.getPath());
  }
#endif

  client_.connect();
}

bool PortmapClient::unsetMapping(PortmapMapping map) {
  return client_.call<bool, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapUnSet, map);
}

bool PortmapClient::setMapping(PortmapMapping map) {
  return client_.call<bool, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapSet, map);
}

uint32_t PortmapClient::getPort(PortmapMapping map) {
  return client_.call<uint32_t, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapGetPort, map);
}

} // namespace facebook::eden
