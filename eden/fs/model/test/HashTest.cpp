/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/model/Hash.h"

#include <folly/String.h>
#include <folly/container/Array.h>
#include <folly/io/Cursor.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using folly::ByteRange;
using folly::IOBuf;
using folly::StringPiece;
using folly::io::Appender;
using std::string;

namespace {
string testHashHex = folly::to<string>(
    "faceb00c",
    "deadbeef",
    "c00010ff",
    "1badb002",
    "8badf00d");

Hash testHash(testHashHex);
} // namespace

TEST(Hash, testDefaultConstructor) {
  EXPECT_EQ("0000000000000000000000000000000000000000", Hash().toString());
}

TEST(Hash, emptySha1) {
  EXPECT_EQ(kEmptySha1, Hash::sha1(IOBuf{}));
}

TEST(Hash, testByteArrayConstructor) {
  EXPECT_EQ(testHashHex, testHash.toString());
}

TEST(Hash, testByteRangeConstructor) {
  auto bytes = folly::make_array<uint8_t>(
      // faceb00c
      0xfa,
      0xce,
      0xb0,
      0x0c,

      // deadbeef
      0xde,
      0xad,
      0xbe,
      0xef,

      // c00010ff
      0xc0,
      0x00,
      0x10,
      0xff,

      // 1badb002
      0x1b,
      0xad,
      0xb0,
      0x02,

      // 8badf00d
      0x8b,
      0xad,
      0xf0,
      0x0d);
  auto byteRange = folly::ByteRange(bytes.data(), bytes.size());
  Hash hash(byteRange);
  EXPECT_EQ(hash, testHash);
  EXPECT_EQ(byteRange, hash.getBytes());
  EXPECT_EQ(hash.getBytes(), testHash.getBytes());
}

TEST(Hash, testCopyConstructor) {
  Hash copyOfTestHash(testHash);
  EXPECT_EQ(testHash.toString(), copyOfTestHash.toString());
  EXPECT_TRUE(testHash == copyOfTestHash);
  EXPECT_FALSE(testHash != copyOfTestHash);
}

TEST(Hash, ensureHashCopiesBytesPassedToConstructor) {
  std::array<uint8_t, 20> bytes = {
      // faceb00c
      0xfa,
      0xce,
      0xb0,
      0x0c,

      // deadbeef
      0xde,
      0xad,
      0xbe,
      0xef,

      // c00010ff
      0xc0,
      0x00,
      0x10,
      0xff,

      // 1badb002
      0x1b,
      0xad,
      0xb0,
      0x02,

      // 8badf00d
      0x8b,
      0xad,
      0xf0,
      0x0d,
  };
  Hash hash1(bytes);
  bytes[0] = 0xc0;
  Hash hash2(bytes);
  EXPECT_EQ("faceb00cdeadbeefc00010ff1badb0028badf00d", hash1.toString());
  EXPECT_EQ("c0ceb00cdeadbeefc00010ff1badb0028badf00d", hash2.toString());
  EXPECT_TRUE(hash1 != hash2);
  EXPECT_TRUE(hash2 < hash1);
  EXPECT_TRUE(hash1 > hash2);
}

TEST(Hash, constexprHexConstructor) {
  // It would be nice to static_assert that two Hashes are equal.
  // Unfortunately the std::array operator== comparison isn't constexpr until
  // C++20, so we don't do this for now.
  static_assert(
      Hash("faceb00cdeadbeefc00010ff1badb0028badf00d").getBytes().data()[0] ==
      0xfa);
  static_assert(
      Hash("faceb00cdeadbeefc00010ff1badb0028badf00d").getBytes().data()[1] ==
      0xce);
  static_assert(
      Hash("faceb00cdeadbeefc00010ff1badb0028badf00d").getBytes().data()[15] ==
      0x02);
}

TEST(Hash, constexprBytesConstructor) {
  constexpr std::array<uint8_t, 20> data = {
      0xfa, 0xce, 0xb0, 0x0c, 0xde, 0xad, 0xbe, 0xef, 0xc0, 0x00,
      0x10, 0xff, 0x1b, 0xad, 0xb0, 0x02, 0x8b, 0xad, 0xf0, 0x0d,
  };
  static_assert(Hash(data).getBytes().data()[0] == 0xfa);
  static_assert(Hash(data).getBytes().data()[1] == 0xce);
  static_assert(Hash(data).getBytes().data()[15] == 0x02);
}

TEST(Hash, ensureStringConstructorRejectsArgumentWithWrongLength) {
  EXPECT_THROW(Hash("badfood"), std::invalid_argument);
}

TEST(Hash, ensureStringConstructorRejectsArgumentBadCharacters) {
  EXPECT_THROW(
      Hash("ZZZZb00cdeadbeefc00010ff1badb0028badf00d"), std::invalid_argument);
}

TEST(Hash, sha1IOBuf) {
  // Test computing the SHA1 of data spread across an IOBuf chain
  auto buf1 = IOBuf::create(50);
  auto buf2 = IOBuf::create(50);
  auto buf3 = IOBuf::create(50);

  // Put some data in the first and third buffer, and leave the second empty
  Appender app(buf1.get(), 0);
  app.push(StringPiece("abcdefghijklmnopqrstuvwxyz1234567890"));
  app = Appender(buf3.get(), 0);
  app.writeBE<uint32_t>(0x00112233);
  app.push(StringPiece("0987654321zyxwvutsrqponmlkjihgfedcba"));

  // Chain them together
  buf2->appendChain(std::move(buf3));
  buf1->appendChain(std::move(buf2));

  EXPECT_EQ(
      Hash("5d105d15efb8b07a624be530ef2b62dab3bc2f8b"), Hash::sha1(*buf1));
}

TEST(Hash, sha1ByteRange) {
  // clang-format off
  auto data = folly::make_array<uint8_t>(
      0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
      0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
      0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
      0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
      0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
      0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
      0x30, 0x31, 0x32, 0x33, 0x34);
  // clang-format on
  EXPECT_EQ(
      Hash("2a9c28ef61eb536d3bbda64ad95a132554be3d6b"),
      Hash::sha1(ByteRange(data.data(), data.size())));
}

TEST(Hash, assignment) {
  Hash h1;
  Hash h2("0123456789abcdeffedcba987654321076543210");
  EXPECT_EQ("0000000000000000000000000000000000000000", h1.toString());
  h1 = h2;
  EXPECT_EQ("0123456789abcdeffedcba987654321076543210", h1.toString());
  EXPECT_EQ(h2, h1);
  h2 = Hash();
  EXPECT_EQ("0000000000000000000000000000000000000000", h2.toString());
}

TEST(Hash, getHashCode) {
  // This isn't so much because we care about the exact value of the hash code,
  // but because we want to make sure that (at least on 64-bit machines), we are
  // using 64 bits of data to contribute to the hash code.
  EXPECT_EQ(folly::Endian::big(0xfaceb00cdeadbeef), testHash.getHashCode());
}
