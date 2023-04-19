/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/portmap/PortmapClient.h"
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
constexpr uint32_t kPortmapVersionNumber = 4;
constexpr uint32_t kPortmapSet = 1;
constexpr uint32_t kPortmapUnSet = 2;
constexpr uint32_t kPortmapGetAddr = 3;
} // namespace

EDEN_XDR_SERDE_IMPL(PortmapMapping, prog, vers, netid, addr, owner);

PortmapClient::PortmapClient()
    : client_(SocketAddress("127.0.0.1", kPortmapPortNumber)) {
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
  // TODO: We should make the portmapper client (or some interface and derived
  // implementation version of it) cross platform. Currently we are
  // registering our rpc servers with the portmapper directly on Windows, and
  // that is easier to do with out the portmapper client. We should put the
  // registration behind a common abstraction. Perhaps We should even teach the
  // port mapper client to speak v2 and register themselves over the socket?
#ifndef _WIN32
  client_.connect();
#endif
}

bool PortmapClient::unsetMapping(PortmapMapping map) {
#ifndef _WIN32
  return client_.call<bool, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapUnSet, map);
#else
  return false;
#endif
}

bool PortmapClient::setMapping(PortmapMapping map) {
#ifndef _WIN32
  return client_.call<bool, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapSet, map);
#else
  return false;
#endif
}

std::string PortmapClient::getAddr(PortmapMapping map) {
#ifndef _WIN32
  return client_.call<std::string, PortmapMapping>(
      kPortmapProgNumber, kPortmapVersionNumber, kPortmapGetAddr, map);
#else
  return "";
#endif
}

} // namespace facebook::eden
