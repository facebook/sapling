/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/container/Array.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include "eden/fs/nfs/xdr/XdrDeSerializer.h"
#include "eden/fs/nfs/xdr/XdrSerializer.h"

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

TEST(XdrSerialize, integers) {
  roundtrip(true, sizeof(int32_t));
  roundtrip(false, sizeof(int32_t));
  roundtrip(uint32_t(123), sizeof(int32_t));
  roundtrip(uint64_t(123123), sizeof(int64_t));
  roundtrip(float(2.5), sizeof(float));
  roundtrip(double(32.5), sizeof(double));
  roundtrip(std::string("hello"), XdrSerializer::roundUp(5) + sizeof(uint32_t));

  std::vector<uint32_t> numbers{1, 2, 3};
  roundtrip(numbers, 4 * sizeof(uint32_t));

  std::vector<uint8_t> u8Numbers{1, 2, 3};
  roundtrip(u8Numbers, sizeof(uint32_t) + XdrSerializer::roundUp(3));

  auto fixedNumbers = folly::make_array<uint32_t>(3, 2, 1);
  roundtrip(fixedNumbers, 3 * sizeof(uint32_t));
}

// This block shows how to implement XDR serialization for a struct
namespace {
struct MySerializableStruct {
  int32_t number;
  std::string str;

  // This is present just for EXPECT_EQ and isn't required
  // for serialization purposes
  bool operator==(const MySerializableStruct& other) const {
    return number == other.number && str == other.str;
  }
};

void serializeXdr(XdrSerializer& xdr, const MySerializableStruct& s) {
  serializeXdr(xdr, s.number);
  serializeXdr(xdr, s.str);
}

void deSerializeXdrInto(XdrDeSerializer& xdr, MySerializableStruct& s) {
  deSerializeXdrInto(xdr, s.number);
  deSerializeXdrInto(xdr, s.str);
}
} // namespace

TEST(XdrSerializer, structs) {
  MySerializableStruct s{123, "hello world"};
  roundtrip(
      s,
      sizeof(s.number) + sizeof(uint32_t) +
          XdrSerializer::roundUp(s.str.size()));
}
