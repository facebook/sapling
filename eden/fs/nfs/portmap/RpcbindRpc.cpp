/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/portmap/RpcbindRpc.h"

namespace facebook::eden {
EDEN_XDR_SERDE_IMPL(PortmapMapping4, prog, vers, netid, addr, owner);

EDEN_XDR_SERDE_IMPL(PortmapMapping2, prog, vers, prot, port);
} // namespace facebook::eden
