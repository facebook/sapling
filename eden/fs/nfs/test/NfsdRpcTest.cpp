/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/NfsdRpc.h"
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

namespace facebook::eden {

using folly::IOBuf;

namespace {
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
} // namespace

struct ResOk {
  int a;
};
EDEN_XDR_SERDE_DECL(ResOk, a);
EDEN_XDR_SERDE_IMPL(ResOk, a);

struct ResFail {
  int b;
};
EDEN_XDR_SERDE_DECL(ResFail, b);
EDEN_XDR_SERDE_IMPL(ResFail, b);

struct FullVariant : public detail::Nfsstat3Variant<ResOk, ResFail> {};

struct EmptyFailVariant : public detail::Nfsstat3Variant<ResOk> {};

TEST(NfsdRpcTest, variant) {
  FullVariant var1{{{nfsstat3::NFS3_OK, ResOk{42}}}};
  roundtrip(var1, 2 * sizeof(uint32_t));

  FullVariant var2{{{nfsstat3::NFS3ERR_PERM, ResFail{10}}}};
  roundtrip(var2, 2 * sizeof(uint32_t));

  EmptyFailVariant var3{{{nfsstat3::NFS3_OK, ResOk{42}}}};
  roundtrip(var3, 2 * sizeof(uint32_t));

  EmptyFailVariant var4{{{nfsstat3::NFS3ERR_PERM, std::monostate{}}}};
  roundtrip(var4, sizeof(uint32_t));
}

} // namespace facebook::eden

#endif
