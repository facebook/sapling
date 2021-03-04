/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/xdr/Xdr.h"
#include <folly/container/Array.h>
#include <gtest/gtest.h>
#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace facebook::eden {

TEST(XdrSerialize, integers) {
  roundtrip(true, sizeof(int32_t));
  roundtrip(false, sizeof(int32_t));
  roundtrip(uint32_t(123), sizeof(int32_t));
  roundtrip(uint64_t(123123), sizeof(int64_t));
  roundtrip(float(2.5), sizeof(float));
  roundtrip(double(32.5), sizeof(double));
  roundtrip(std::string("hello"), detail::roundUp(5) + sizeof(uint32_t));

  std::vector<uint32_t> numbers{1, 2, 3};
  roundtrip(numbers, 4 * sizeof(uint32_t));

  std::vector<uint8_t> u8Numbers{1, 2, 3};
  roundtrip(u8Numbers, sizeof(uint32_t) + detail::roundUp(3));

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
} // namespace

template <>
struct XdrTrait<MySerializableStruct> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const MySerializableStruct& value) {
    XdrTrait<int32_t>::serialize(appender, value.number);
    XdrTrait<std::string>::serialize(appender, value.str);
  }

  static MySerializableStruct deserialize(folly::io::Cursor& cursor) {
    auto number = XdrTrait<int32_t>::deserialize(cursor);
    auto str = XdrTrait<std::string>::deserialize(cursor);
    return {number, std::move(str)};
  }
};

TEST(XdrSerialize, structs) {
  MySerializableStruct s{123, "hello world"};
  roundtrip(
      s, sizeof(s.number) + sizeof(uint32_t) + detail::roundUp(s.str.size()));
}

struct MyVariant : XdrVariant<bool, uint32_t> {};

template <>
struct XdrTrait<MyVariant> : public XdrTrait<MyVariant::Base> {
  static MyVariant deserialize(folly::io::Cursor& cursor) {
    MyVariant var;
    var.tag = XdrTrait<bool>::deserialize(cursor);
    if (var.tag) {
      var.v = XdrTrait<uint32_t>::deserialize(cursor);
    }
    return var;
  }
};

TEST(XdrSerialize, variant) {
  MyVariant var1{{true, 42u}};
  roundtrip(var1, 2 * sizeof(uint32_t));

  MyVariant var2;
  roundtrip(var2, sizeof(uint32_t));
}

struct OptionalVariant : public XdrOptionalVariant<uint32_t> {};

enum class TestEnum {
  FOO = 0,
  BAR = 1,
};

struct OptionalEnumVariant
    : public XdrOptionalVariant<uint32_t, TestEnum, TestEnum::BAR> {};

TEST(XdrSerialize, optionalVariant) {
  OptionalVariant var1{{42}};
  roundtrip(var1, 2 * sizeof(uint32_t));

  OptionalVariant var2;
  roundtrip(var2, sizeof(uint32_t));

  OptionalEnumVariant opt1{42u};
  EXPECT_EQ(opt1.tag, TestEnum::BAR);
  EXPECT_EQ(std::get<uint32_t>(opt1.v), 42u);
  roundtrip(opt1, 2 * sizeof(uint32_t));

  OptionalEnumVariant opt2;
  EXPECT_EQ(opt2.tag, TestEnum::FOO);
  EXPECT_EQ(std::get<std::monostate>(opt2.v), std::monostate{});
  roundtrip(opt2, sizeof(uint32_t));
}

struct IOBufStruct {
  uint32_t before;
  std::unique_ptr<folly::IOBuf> buf;
  uint32_t after;

  bool operator==(const IOBufStruct& other) const {
    return before == other.before && after == other.after &&
        folly::IOBufEqualTo()(buf, other.buf);
  }
};

template <>
struct XdrTrait<IOBufStruct> {
  static void serialize(
      folly::io::QueueAppender& appender,
      const IOBufStruct& value) {
    XdrTrait<uint32_t>::serialize(appender, value.before);
    XdrTrait<std::unique_ptr<folly::IOBuf>>::serialize(appender, value.buf);
    XdrTrait<uint32_t>::serialize(appender, value.after);
  }

  static IOBufStruct deserialize(folly::io::Cursor& cursor) {
    IOBufStruct ret;
    ret.before = XdrTrait<uint32_t>::deserialize(cursor);
    ret.buf = XdrTrait<std::unique_ptr<folly::IOBuf>>::deserialize(cursor);
    ret.after = XdrTrait<uint32_t>::deserialize(cursor);
    return ret;
  }
};

TEST(XdrSerialize, iobuf) {
  struct IOBufStruct buf {
    42, folly::IOBuf::copyBuffer("This is a test"), 10
  };
  auto bufLen = buf.buf->computeChainDataLength();
  roundtrip(std::move(buf), 3 * sizeof(uint32_t) + bufLen + 2 /*padding*/);
}

} // namespace facebook::eden

#endif
