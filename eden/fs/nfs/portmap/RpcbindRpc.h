/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"

/*
 * Rpcbind prococol version 4 described in section 2 and portmapper (rpcbind
 * version 2) is in section 3  of RFC1833:
 * https://www.rfc-editor.org/rfc/rfc1833
 */

namespace facebook::eden {
constexpr uint32_t kPortmapPortNumber = 111;
constexpr uint32_t kPortmapProgNumber = 100000;
// this is the latest version what we want to use as a client on linux and mac
constexpr uint32_t kPortmapVersion4 = 4;
// as a server we have to support this version, because that is what the msft
// client wants to use.
constexpr uint32_t kPortmapVersion2 = 2;

/**
 * Procedure values.
 */
enum class rpcbindProcs4 : uint32_t {
  null = 0,
  set = 1,
  unset = 2,
  getaddr = 3,
  dump = 4,
  bcast = 5,
  gettime = 6,
  uaddr2taddr = 7,
  taddr2uaddr = 8,
  getversaddr = 9,
  indirect = 10,
  getaddrlist = 11,
  getstat = 12
};

enum class rpcbindProcs2 : uint32_t {
  null = 0,
  set = 1,
  unset = 2,
  getport = 3,
  dump = 4,
  callit = 5,
};

// argument to set, unset, getaddr
struct PortmapMapping4 {
  uint32_t prog;
  uint32_t vers;

  std::string netid;
  std::string addr;
  std::string owner;

  static constexpr const char* kTcpNetId = "tcp";
  static constexpr const char* kTcp6NetId = "tcp6";
  static constexpr const char* kLocalNetId = "local";
};
EDEN_XDR_SERDE_DECL(PortmapMapping4, prog, vers, netid, addr, owner);

struct PortmapMapping2 {
  uint32_t prog;
  uint32_t vers;
  uint32_t prot;
  uint32_t port;

  static const uint32_t kTcpProto = 6; /* protocol number for TCP/IP */
  static const uint32_t kUdpProto = 17; /* protocol number for UDP/IP */
};
EDEN_XDR_SERDE_DECL(PortmapMapping2, prog, vers, prot, port);

} // namespace facebook::eden
