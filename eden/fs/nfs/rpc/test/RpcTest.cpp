/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

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
  folly::io::Appender appender(&buf, kDefaultBufferSize);
  XdrTrait<T>::serialize(appender, t);
  return buf;
}

template <typename T>
T de(IOBuf buf) {
  folly::io::Cursor cursor(&buf);
  auto ret = XdrTrait<T>::deserialize(cursor);
  if (!cursor.isAtEnd()) {
    throw std::runtime_error(folly::to<std::string>(
        "unexpected trailing bytes (", cursor.totalLength(), ")"));
  }
  return ret;
}

// Validates that `T` can be serialized into something of an expected
// encoded size and deserialized back to something that compares
// equal to the original value
template <typename T>
void roundtrip(T value, size_t encodedSize) {
  auto encoded = ser(value);
  EXPECT_EQ(encoded.coalesce().size(), encodedSize);
  auto decoded = de<T>(encoded);
  EXPECT_EQ(value, decoded);
}

TEST(RpcTest, enums) {
  roundtrip(auth_flavor::AUTH_NONE, sizeof(int32_t));
  roundtrip(opaque_auth{}, 2 * sizeof(uint32_t));

  roundtrip(
      rejected_reply{{reject_stat::RPC_MISMATCH, mismatch_info{0, 1}}},
      sizeof(mismatch_info) + sizeof(uint32_t));
  roundtrip(
      rejected_reply{{reject_stat::AUTH_ERROR, auth_stat::AUTH_FAILED}},
      sizeof(auth_stat) + sizeof(uint32_t));
}

#endif
