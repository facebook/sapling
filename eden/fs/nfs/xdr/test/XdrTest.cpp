/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/xdr/Xdr.h"
#include <folly/container/Array.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/memory/MallctlHelper.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include <sys/resource.h>
#include <stdexcept>
#include "eden/fs/nfs/NfsdRpc.h"
#include "eden/fs/nfs/rpc/Rpc.h"
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

TEST(XdrDeserialize, lengthPrefixBeyondBuffer) {
  for (auto prefix :
       {std::string("\x00\x10\x00\x00", 4),
        std::string("\xff\xff\xff\xf0", 4)}) {
    auto buf = folly::IOBuf::copyBuffer(prefix + std::string(8, 'x'));
    {
      folly::io::Cursor cursor(buf.get());
      EXPECT_THROW_RE(
          XdrTrait<std::vector<uint8_t>>::deserialize(cursor),
          std::out_of_range,
          "exceeds");
    }
    {
      folly::io::Cursor cursor(buf.get());
      EXPECT_THROW_RE(
          XdrTrait<std::string>::deserialize(cursor),
          std::out_of_range,
          "exceeds");
    }
    {
      folly::io::Cursor cursor(buf.get());
      EXPECT_THROW_RE(
          XdrTrait<std::vector<uint32_t>>::deserialize(cursor),
          std::out_of_range,
          "exceeds");
    }
  }
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
  struct IOBufStruct buf{42, folly::IOBuf::copyBuffer("This is a test"), 10};
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

namespace {

/**
 * Bytes this thread has requested from the allocator, or 0 when the binary is
 * not linked against jemalloc, which is the only allocator here that exports
 * the counter. This is what sees a reserve(), which never touches the pages it
 * obtains and so leaves no trace in resident memory.
 */
uint64_t threadAllocatedBytes() {
  if (!folly::usingJEMalloc()) {
    return 0;
  }
  uint64_t allocated = 0;
  folly::mallctlRead("thread.allocated", &allocated);
  return allocated;
}

/**
 * Peak resident set of the process, in kilobytes. A high water mark, so it
 * still reports an allocation that has since been freed. It also climbs on its
 * own as a young process touches pages, so only a large jump means anything.
 */
uint64_t peakRssKb() {
  rusage usage{};
  getrusage(RUSAGE_SELF, &usage);
#ifdef __APPLE__
  // macOS reports ru_maxrss in bytes; Linux reports kilobytes.
  return static_cast<uint64_t>(usage.ru_maxrss) / 1024;
#else
  return static_cast<uint64_t>(usage.ru_maxrss);
#endif
}

/**
 * Decode a T out of a variable-length field that declares almost 4GB but is
 * followed by a single 4-byte word, and report what that cost in bytes
 * requested from the allocator and in peak resident kilobytes.
 */
template <typename T>
std::pair<uint64_t, uint64_t> costOfOverDeclaredLength() {
  folly::IOBufQueue queue;
  folly::io::QueueAppender appender(&queue, 8);
  appender.writeBE<uint32_t>(0xfffffff0);
  appender.writeBE<uint32_t>(0);
  auto wire = queue.move();

  folly::io::Cursor cursor(wire.get());
  auto allocatedBefore = threadAllocatedBytes();
  auto rssBefore = peakRssKb();
  EXPECT_THROW(XdrTrait<T>::deserialize(cursor), std::out_of_range);
  return {threadAllocatedBytes() - allocatedBefore, peakRssKb() - rssBefore};
}

} // namespace

TEST(XdrDeserialize, overDeclaredLengthDoesNotAllocate) {
  constexpr uint64_t kMaxBytes = 1 << 20;
  constexpr uint64_t kMaxRssKb = 64 << 10;

  auto [vecBytes, vecRssKb] = costOfOverDeclaredLength<std::vector<uint8_t>>();
  EXPECT_LT(vecBytes, kMaxBytes);
  EXPECT_LT(vecRssKb, kMaxRssKb);

  auto [strBytes, strRssKb] = costOfOverDeclaredLength<std::string>();
  EXPECT_LT(strBytes, kMaxBytes);
  EXPECT_LT(strRssKb, kMaxRssKb);

  auto [listBytes, listRssKb] =
      costOfOverDeclaredLength<std::vector<uint32_t>>();
  EXPECT_LT(listBytes, kMaxBytes);
  EXPECT_LT(listRssKb, kMaxRssKb);
}

TEST(XdrDeserialize, lengthCheckSpansTheIOBufChain) {
  const std::string text{"hello world!"};
  std::vector<uint8_t> wire{0, 0, 0, static_cast<uint8_t>(text.size())};
  wire.insert(wire.end(), text.begin(), text.end());

  // Straddle the payload across two buffers, which a bound taken from the
  // current buffer alone would reject even though the message is well formed.
  constexpr size_t kSplit = 6;
  auto head = folly::IOBuf::wrapBufferAsValue(wire.data(), kSplit);
  head.prependChain(
      folly::IOBuf::wrapBuffer(wire.data() + kSplit, wire.size() - kSplit));

  folly::io::Cursor cursor(&head);
  EXPECT_EQ(text, XdrTrait<std::string>::deserialize(cursor));
}

template <typename T>
T deBytes(std::initializer_list<uint8_t> bytes) {
  return de<T>(IOBuf::copyBuffer(std::data(bytes), bytes.size()));
}

// Minimized libFuzzer inputs from T290252410. Each one declares a length in
// the gigabytes inside a 16 or 32 byte message.
TEST(XdrDeserialize, fuzzRpcCallOpaqueAuthBody) {
  EXPECT_THROW(
      deBytes<rpc_msg_call>({0x00, 0x02, 0x00, 0x2b, 0x0a, 0x00, 0x02, 0x00,
                             0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5,
                             0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5,
                             0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0xf5, 0x0a}),
      std::out_of_range);
}

TEST(XdrDeserialize, fuzzLookupArgsName) {
  EXPECT_THROW(
      deBytes<LOOKUP3args>(
          {0x00,
           0x00,
           0x00,
           0x08,
           0x0a,
           0x0a,
           0xff,
           0xff,
           0xff,
           0x0a,
           0x00,
           0x0a,
           0xff,
           0xff,
           0xff,
           0x0a}),
      std::out_of_range);
}

TEST(XdrDeserialize, fuzzCreateArgsName) {
  EXPECT_THROW(
      deBytes<CREATE3args>(
          {0x00,
           0x00,
           0x00,
           0x08,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff,
           0xff}),
      std::out_of_range);
}

} // namespace facebook::eden

#endif
