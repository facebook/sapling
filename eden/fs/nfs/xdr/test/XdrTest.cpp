/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/xdr/Xdr.h"
#include <folly/container/Array.h>
#include <folly/portability/GTest.h>
#include "eden/fs/nfs/testharness/XdrTestUtils.h"

namespace facebook::eden {

TEST(XdrSerialize, integers) {
  roundtrip(true);
  roundtrip(false);
  roundtrip(uint32_t(123));
  roundtrip(uint64_t(123123));
  roundtrip(float(2.5));
  roundtrip(double(32.5));
  roundtrip(std::string("hello"));

  std::vector<uint32_t> numbers{1, 2, 3};
  roundtrip(numbers);

  std::vector<uint8_t> u8Numbers{1, 2, 3};
  roundtrip(u8Numbers);

  auto fixedNumbers = folly::make_array<uint32_t>(3, 2, 1);
  roundtrip(fixedNumbers);
}

struct MySerializableStruct {
  int32_t number;
  std::string str;
};
EDEN_XDR_SERDE_DECL(MySerializableStruct, number, str);
EDEN_XDR_SERDE_IMPL(MySerializableStruct, number, str);

TEST(XdrSerialize, structs) {
  MySerializableStruct s{123, "hello world"};
  roundtrip(s);
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
  roundtrip(var1);

  MyVariant var2;
  roundtrip(var2);
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
  roundtrip(var1);

  OptionalVariant var2;
  roundtrip(var2);

  OptionalEnumVariant opt1{42u};
  EXPECT_EQ(opt1.tag, TestEnum::BAR);
  EXPECT_EQ(std::get<uint32_t>(opt1.v), 42u);
  roundtrip(opt1);

  OptionalEnumVariant opt2;
  EXPECT_EQ(opt2.tag, TestEnum::FOO);
  EXPECT_EQ(std::get<std::monostate>(opt2.v), std::monostate{});
  roundtrip(opt2);
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

// We can't use EDEN_XDR_SERDE_DECL as it generates code that compares
// unique_ptr and not their contents.
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

  static size_t serializedSize(const IOBufStruct& value) {
    return 2 * XdrTrait<uint32_t>::serializedSize(0) +
        XdrTrait<std::unique_ptr<folly::IOBuf>>::serializedSize(value.buf);
  }
};

TEST(XdrSerialize, iobuf) {
  struct IOBufStruct buf {
    42, folly::IOBuf::copyBuffer("This is a test"), 10
  };
  roundtrip(std::move(buf));
}

struct ListElement {
  uint32_t value;
};
EDEN_XDR_SERDE_DECL(ListElement, value);
EDEN_XDR_SERDE_IMPL(ListElement, value);

struct ListHead {
  XdrList<ListElement> elements;
};
EDEN_XDR_SERDE_DECL(ListHead, elements);
EDEN_XDR_SERDE_IMPL(ListHead, elements);

TEST(XdrSerialize, list) {
  std::vector<ListElement> elements;
  elements.emplace_back(ListElement{1});
  elements.emplace_back(ListElement{2});
  elements.emplace_back(ListElement{3});
  elements.emplace_back(ListElement{4});

  ListHead head{{std::move(elements)}};

  roundtrip(head);
}

TEST(XdrSerialize, optional) {
  std::optional<uint32_t> nullOpt{std::nullopt};
  roundtrip(nullOpt);

  std::optional<uint32_t> answerOpt{42};
  roundtrip(answerOpt);
}

} // namespace facebook::eden

#endif
