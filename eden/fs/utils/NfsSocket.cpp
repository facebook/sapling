/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/NfsSocket.h"

#include <folly/Exception.h>

namespace facebook::eden {

folly::SocketAddress makeNfsSocket(std::optional<AbsolutePath> unixSocketPath) {
  if (folly::kIsApple && unixSocketPath.has_value()) {
    int rc = unlink(unixSocketPath->c_str());
    if (rc != 0 && errno != ENOENT) {
      folly::throwSystemError(
          fmt::format("unable to remove socket file {}", *unixSocketPath));
    }
    return folly::SocketAddress::makeFromPath(unixSocketPath->stringPiece());
  } else {
    return folly::SocketAddress("127.0.0.1", 0);
  }
}

} // namespace facebook::eden
