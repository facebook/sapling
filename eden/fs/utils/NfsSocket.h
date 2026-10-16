/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Portability.h>
#include <folly/SocketAddress.h>
#include <optional>
#include <string>
#include <vector>
#include "eden/common/utils/PathFuncs.h"

namespace facebook::eden {

/**
 * The name of a checkout's nfsd socket, in its client directory. The name is
 * part of how the mount is recognized: macOS reports a mount served over a
 * Unix domain socket as "<socket path>:<mount path>", see
 * is_edenfs_nfs_mount().
 */
constexpr PathComponentPiece kNfsdSocketName{"edenfs-nfsd.socket"_pc};

/**
 * The longest path a Unix domain socket can have on this platform: the size
 * of sockaddr_un's sun_path less its terminator.
 */
constexpr size_t kMaxUnixSocketPathLength = folly::kIsApple ? 103 : 107;

/**
 * The checkout name length that whyNfsCannotUseUnixSockets() keeps room for,
 * so that a checkout cloned after the daemon has committed to Unix sockets
 * still fits when its name is of ordinary length, such as "fbsource-123456".
 */
constexpr size_t kReservedCheckoutNameLength = 16;

/**
 * Why the NFS sockets of a state directory cannot be Unix domain sockets, or
 * nullopt when they can. The kernel requires a checkout's mountd and nfsd
 * sockets to share a transport and the mountd is shared by every checkout,
 * so this is decided once for the daemon, before the mountd is bound: the
 * mountd socket, the nfsd socket of every configured checkout and that of a
 * checkout with a name of kReservedCheckoutNameLength bytes must all fit.
 */
std::optional<std::string> whyNfsCannotUseUnixSockets(
    AbsolutePathPiece mountdSocketPath,
    AbsolutePathPiece clientsDir,
    const std::vector<std::string>& checkoutIds);

folly::SocketAddress makeNfsSocket(std::optional<AbsolutePath> unixSocketPath);

} // namespace facebook::eden
