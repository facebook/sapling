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

folly::SocketAddress makeNfsSocket(std::optional<AbsolutePath> unixSocketPath) {
  if (folly::kIsApple && unixSocketPath.has_value()) {
    // sockaddr_un holds 104 bytes of path on macOS, and the socket file lives
    // under the checkout's state directory, so a deeply nested state
    // directory can run out of room. Name the path and the limit rather than
    // let folly's generic error surface as a mount failure.
    constexpr size_t kMaxUnixSocketPath = 104;
    if (unixSocketPath->view().size() >= kMaxUnixSocketPath) {
      throw std::invalid_argument(
          fmt::format(
              "NFS socket path {} is {} bytes long; Unix socket paths on this "
              "platform are limited to {} bytes. Use a shorter EdenFS state "
              "directory.",
              *unixSocketPath,
              unixSocketPath->view().size(),
              kMaxUnixSocketPath - 1));
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
