/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/ObjectId.h"

#include <folly/Range.h>
#include <folly/container/Array.h>
#include <gtest/gtest.h>

namespace {

using namespace facebook::eden;
using folly::ByteRange;

TEST(ObjectId, testHashCodeExact) {
  auto bytes = folly::make_array<uint8_t>(
      0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0xff, 0xff);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto exactObjectId = ObjectId(byteRange);
  auto hashCode = exactObjectId.getHashCode();
  EXPECT_EQ(hashCode, folly::Endian::big(0x0000ffff0000ffff));
}

TEST(ObjectId, testHashCodeShort) {
  auto bytes = folly::make_array<uint8_t>(0x00, 0xff);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto shortObjectId = ObjectId(byteRange);
  auto hashCode = shortObjectId.getHashCode();
  // May not work correctly on little-endian machines.
  EXPECT_EQ(hashCode, 0xff00);
}

TEST(ObjectId, testHashCodeLong) {
  auto bytes = folly::make_array<uint8_t>(
      // all 1s in binary
      0x01,
      0x01,
      0x01,
      0x01,

      0x01,
      0x01,
      0x01,
      0x01,

      0x02,
      0x02,
      0x02,
      0x02,

      0x02,
      0x02,
      0x02,
      0x02,

      0x04,
      0x04,
      0x04,
      0x04,

      0x04,
      0x04,
      0x04,
      0x04);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto longObjectId = ObjectId(byteRange);
  auto hashCode = longObjectId.getHashCode();
  EXPECT_EQ(hashCode, folly::Endian::big(0x0707070707070707));
}

TEST(ObjectId, testHashCodeNotMod8) {
  auto bytes = folly::make_array<uint8_t>(
      // all 1s in binary
      0xff,
      0xff,
      0xff,
      0xff,

      0xff,
      0xff,
      0xff,
      0xff,

      // all 0s in binary
      0x00,
      0x00,
      0x00,
      0x00);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto notMod8ObjectId = ObjectId(byteRange);
  auto hashCode = notMod8ObjectId.getHashCode();

  // When length of ObjectID is not a multiple of 8, we end up overlapping
  // xor byte ranges. In this case, we'll xor as follows:
  //
  // 0x00 00 00 00 ff ff ff ff
  // 0xff ff ff ff ff ff ff ff ^
  // --------------------------
  // 0xff ff ff ff 00 00 00 00
  //
  EXPECT_EQ(hashCode, 0xffffffff00000000);
}

TEST(ObjectId, testFormattingHashCodeExact) {
  auto bytes = folly::make_array<uint8_t>(
      0x00, 0x00, 0xff, 0xff, 0x00, 0x00, 0xff, 0xff);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto exactObjectId = ObjectId(byteRange);
  EXPECT_EQ("0000ffff0000ffff", fmt::to_string(exactObjectId));
}

TEST(ObjectId, testFormattingHashCodeShort) {
  auto bytes = folly::make_array<uint8_t>(0x00, 0xff);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto shortObjectId = ObjectId(byteRange);
  EXPECT_EQ("00ff", fmt::to_string(shortObjectId));
}

TEST(ObjectId, testFormattingHashCodeLong) {
  auto bytes = folly::make_array<uint8_t>(
      0x01,
      0x01,
      0x01,
      0x01,

      0x01,
      0x01,
      0x01,
      0x01,

      0x02,
      0x02,
      0x02,
      0x02,

      0x02,
      0x02,
      0x02,
      0x02,

      0x04,
      0x04,
      0x04,
      0x04,

      0x04,
      0x04,
      0x04,
      0x04);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto longObjectId = ObjectId(byteRange);
  EXPECT_EQ(
      "010101010101010102020202020202020404040404040404",
      fmt::to_string(longObjectId));
}

TEST(ObjectId, testFormattingHashCodeNotMod8) {
  auto bytes = folly::make_array<uint8_t>(
      // all 1s in binary
      0xff,
      0xff,
      0xff,
      0xff,

      0xff,
      0xff,
      0xff,
      0xff,

      // all 0s in binary
      0x00,
      0x00,
      0x00,
      0x00);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  auto notMod8ObjectId = ObjectId(byteRange);
  EXPECT_EQ("ffffffffffffffff00000000", fmt::to_string(notMod8ObjectId));
}
} // namespace
