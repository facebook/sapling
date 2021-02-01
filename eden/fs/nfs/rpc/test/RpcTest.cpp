/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/Rpc.h"
#include <folly/container/Array.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using folly::IOBuf;

template <typename T>
IOBuf ser(const T& t) {
  constexpr size_t kDefaultBufferSize = 1024;
  IOBuf buf(IOBuf::CREATE, kDefaultBufferSize);
  XdrSerializer appender(&buf, kDefaultBufferSize);
  serializeXdr(appender, t);
  return buf;
}

template <typename T>
void de(IOBuf buf, T& value) {
  XdrDeSerializer xdr(&buf);
  deSerializeXdrInto(xdr, value);
  if (!xdr.isAtEnd()) {
    throw std::runtime_error(folly::to<std::string>(
        "unexpected trailing bytes (", xdr.totalLength(), ")"));
  }
}

// Validates that `T` can be serialized into something of an expected
// encoded size and deserialized back to something that compares
// equal to the original value
template <typename T>
void roundtrip(T value, size_t encodedSize) {
  auto encoded = ser(value);
  EXPECT_EQ(encoded.coalesce().size(), encodedSize);
  T decoded;
  de(encoded, decoded);
  EXPECT_EQ(value, decoded);
}

TEST(RpcTest, enums) {
  roundtrip(rpc::auth_flavor::AUTH_NONE, sizeof(int32_t));
  roundtrip(rpc::opaque_auth{}, 2 * sizeof(uint32_t));

  roundtrip(
      rpc::rejected_reply{
          rpc::reject_stat::RPC_MISMATCH, rpc::mismatch_info{0, 1}},
      sizeof(rpc::mismatch_info) + sizeof(uint32_t));
  roundtrip(
      rpc::rejected_reply{
          rpc::reject_stat::AUTH_ERROR, rpc::auth_stat::AUTH_FAILED},
      sizeof(rpc::auth_stat) + sizeof(uint32_t));
}
