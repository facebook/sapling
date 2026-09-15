/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/testharness/NfsRequestUtils.h"

namespace facebook::eden {

opaque_auth makeAuthSysCred(const authsys_parms& creds) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender ser(&queue, 256);
  XdrTrait<authsys_parms>::serialize(ser, creds);
  auto buf = queue.move();
  auto bytes = buf->coalesce();
  return opaque_auth{
      auth_flavor::AUTH_SYS, OpaqueBytes{bytes.begin(), bytes.end()}};
}

} // namespace facebook::eden
