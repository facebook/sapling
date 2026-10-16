/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/NfsSocket.h"

#include <fmt/format.h>
#include <folly/Exception.h>

#include <stdexcept>

namespace facebook::eden {

namespace {
std::optional<std::string> describeIfTooLong(
    std::string_view what,
    std::string_view path) {
  if (path.size() <= kMaxUnixSocketPathLength) {
    return std::nullopt;
  }
  return fmt::format(
      "{} {} is {} bytes long; Unix socket paths on this platform are limited "
      "to {} bytes",
      what,
      path,
      path.size(),
      kMaxUnixSocketPathLength);
}

std::string nfsdSocketPathFor(
    AbsolutePathPiece clientsDir,
    std::string_view checkoutId) {
  return fmt::format(
      "{}/{}/{}", clientsDir, checkoutId, kNfsdSocketName.view());
}
} // namespace

std::optional<std::string> whyNfsCannotUseUnixSockets(
    AbsolutePathPiece mountdSocketPath,
    AbsolutePathPiece clientsDir,
    const std::vector<std::string>& checkoutIds) {
  if (auto why = describeIfTooLong(
          "the mountd socket path", mountdSocketPath.view())) {
    return why;
  }
  for (const auto& checkoutId : checkoutIds) {
    if (auto why = describeIfTooLong(
            "the nfsd socket path",
            nfsdSocketPathFor(clientsDir, checkoutId))) {
      return why;
    }
  }
  return describeIfTooLong(
      fmt::format(
          "the nfsd socket path of a checkout with a {}-byte name",
          kReservedCheckoutNameLength),
      nfsdSocketPathFor(
          clientsDir, std::string(kReservedCheckoutNameLength, 'x')));
}

folly::SocketAddress makeNfsSocket(std::optional<AbsolutePath> unixSocketPath) {
  if (folly::kIsApple && unixSocketPath.has_value()) {
    // The daemon serves NFS over TCP when it can see at startup that a socket
    // path would not fit, so this is reached by a checkout cloned since then
    // with a name longer than the daemon kept room for. Name the path and the
    // limit rather than let folly's generic error surface as a mount failure.
    if (auto why =
            describeIfTooLong("The NFS socket path", unixSocketPath->view())) {
      throw std::invalid_argument(
          fmt::format(
              "{}. Run `eden restart` to serve NFS over TCP instead.", *why));
    }
    int rc = unlink(unixSocketPath->c_str());
    if (rc != 0 && errno != ENOENT) {
      folly::throwSystemError(
          fmt::format("unable to remove socket file {}", *unixSocketPath));
    }
    return folly::SocketAddress::makeFromPath(unixSocketPath->view());
  } else {
    return folly::SocketAddress("127.0.0.1", 0);
  }
}

} // namespace facebook::eden
