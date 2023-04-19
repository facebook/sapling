/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/net/NetworkSocket.h>
#include "eden/fs/nfs/portmap/RpcbindRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/nfs/rpc/StreamClient.h"

// Implement: https://tools.ietf.org/html/rfc1833

namespace facebook::eden {

class PortmapClient {
 public:
  bool setMapping(PortmapMapping4 map);
  bool unsetMapping(PortmapMapping4 map);
  std::string getAddr(PortmapMapping4 map);

  PortmapClient();

 private:
#ifdef __APPLE__
  folly::NetworkSocket tickler_;
#endif
  StreamClient client_;
};

} // namespace facebook::eden
