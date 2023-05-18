/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/model/ObjectId.h"

#include <folly/Range.h>
#include <folly/container/Array.h>
#include <folly/portability/GTest.h>

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
  auto exactObjectId = ObjectId(byteRange);
  auto hashCode = exactObjectId.getHashCode();
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
  auto exactObjectId = ObjectId(byteRange);
  auto hashCode = exactObjectId.getHashCode();
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
  auto exactObjectId = ObjectId(byteRange);
  auto hashCode = exactObjectId.getHashCode();

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
} // namespace
