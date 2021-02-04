/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/net/NetworkSocket.h>
#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/nfs/rpc/StreamClient.h"

namespace facebook::eden {

struct PortmapMapping {
  uint32_t prog;
  uint32_t vers;
  uint32_t prot;
  uint32_t port;

  static constexpr uint32_t kProtoTcp = 6;
  static constexpr uint32_t kProtoUdp = 17;
};
EDEN_XDR_SERDE_DECL(PortmapMapping, prog, vers, prot, port);

class PortmapClient {
 public:
  bool setMapping(PortmapMapping map);
  bool unsetMapping(PortmapMapping map);
  uint32_t getPort(PortmapMapping map);

  PortmapClient();

 private:
#ifdef __APPLE__
  folly::NetworkSocket tickler_;
#endif
  StreamClient client_;
};

} // namespace facebook::eden
