/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/Rpc.h"

#include <gtest/gtest.h>

#include <folly/io/IOBufQueue.h>

#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace {

using namespace facebook::eden;

TEST(RpcTest, enums) {
  roundtrip(auth_flavor::AUTH_NONE);
  roundtrip(opaque_auth{});

  roundtrip(rejected_reply{{reject_stat::RPC_MISMATCH, mismatch_info{0, 1}}});
  roundtrip(rejected_reply{{reject_stat::AUTH_ERROR, auth_stat::AUTH_FAILED}});
}

OpaqueBytes serializeAuthSysBody(const authsys_parms& creds) {
  folly::IOBufQueue queue{folly::IOBufQueue::cacheChainLength()};
  folly::io::QueueAppender appender{&queue, 1024};
  XdrTrait<authsys_parms>::serialize(appender, creds);
  auto buf = queue.move();
  auto bytes = buf->coalesce();
  return OpaqueBytes{bytes.begin(), bytes.end()};
}

TEST(RpcTest, parseAuthSysCredsRoundTrip) {
  authsys_parms creds{/*stamp=*/42,
                      /*machinename=*/"testhost",
                      /*uid=*/0,
                      /*gid=*/0,
                      /*gids=*/{0, 1, 2}};
  opaque_auth auth{auth_flavor::AUTH_SYS, serializeAuthSysBody(creds)};

  auto parsed = parseAuthSysCreds(auth);
  ASSERT_TRUE(parsed.has_value());
  EXPECT_EQ(parsed->stamp, 42u);
  EXPECT_EQ(parsed->machinename, "testhost");
  EXPECT_EQ(parsed->uid, 0u);
  EXPECT_EQ(parsed->gid, 0u);
  EXPECT_EQ(parsed->gids, (std::vector<uint32_t>{0, 1, 2}));
}

TEST(RpcTest, parseAuthSysCredsWrongFlavor) {
  authsys_parms creds{1, "host", 501, 20, {}};
  auto body = serializeAuthSysBody(creds);

  EXPECT_EQ(
      parseAuthSysCreds(opaque_auth{auth_flavor::AUTH_NONE, body}),
      std::nullopt);
  EXPECT_EQ(
      parseAuthSysCreds(opaque_auth{auth_flavor::AUTH_DH, body}), std::nullopt);
}

TEST(RpcTest, parseAuthSysCredsTruncated) {
  authsys_parms creds{1, "host", 501, 20, {20, 12}};
  auto body = serializeAuthSysBody(creds);

  // Every possible truncation must yield nullopt without crashing.
  for (size_t len = 0; len < body.size(); len++) {
    OpaqueBytes truncated{body.begin(), body.begin() + len};
    EXPECT_EQ(
        parseAuthSysCreds(opaque_auth{auth_flavor::AUTH_SYS, truncated}),
        std::nullopt)
        << "body truncated to " << len << " bytes";
  }
}

TEST(RpcTest, parseAuthSysCredsHostileLengths) {
  // A machinename length prefix far larger than the body must be rejected
  // without attempting a giant allocation.
  OpaqueBytes bogusName{
      0,
      0,
      0,
      1, // stamp
      0xff,
      0xff,
      0xff,
      0xff, // machinename length
  };
  EXPECT_EQ(
      parseAuthSysCreds(opaque_auth{auth_flavor::AUTH_SYS, bogusName}),
      std::nullopt);

  // Same for the gids count: RFC 5531 caps it at 16 entries.
  authsys_parms creds{1, "", 501, 20, {}};
  auto body = serializeAuthSysBody(creds);
  // Overwrite the trailing gids length (last 4 bytes) with a huge count.
  body[body.size() - 4] = 0xff;
  body[body.size() - 3] = 0xff;
  body[body.size() - 2] = 0xff;
  body[body.size() - 1] = 0xff;
  EXPECT_EQ(
      parseAuthSysCreds(opaque_auth{auth_flavor::AUTH_SYS, body}),
      std::nullopt);
}

} // namespace
