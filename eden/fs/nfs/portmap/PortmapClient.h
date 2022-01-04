/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#ifndef _WIN32

#include <folly/net/NetworkSocket.h>
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/nfs/rpc/StreamClient.h"

// Implement: https://tools.ietf.org/html/rfc1833

namespace facebook::eden {

struct PortmapMapping {
  uint32_t prog;
  uint32_t vers;

  std::string netid;
  std::string addr;
  std::string owner;

  static constexpr const char* kTcpNetId = "tcp";
  static constexpr const char* kTcp6NetId = "tcp6";
  static constexpr const char* kLocalNetId = "local";
};
EDEN_XDR_SERDE_DECL(PortmapMapping, prog, vers, netid, addr, owner);

class PortmapClient {
 public:
  bool setMapping(PortmapMapping map);
  bool unsetMapping(PortmapMapping map);
  std::string getAddr(PortmapMapping map);

  PortmapClient();

 private:
#ifdef __APPLE__
  folly::NetworkSocket tickler_;
#endif
  StreamClient client_;
};

} // namespace facebook::eden

#endif
