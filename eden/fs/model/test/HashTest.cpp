/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/Hash.h"

#include <folly/String.h>
#include <gtest/gtest.h>

using facebook::eden::Hash;
using std::string;

string testHashHex = folly::to<string>(
    "faceb00c",
    "deadbeef",
    "c00010ff",
    "1badb002",
    "8badf00d");

Hash testHash(testHashHex);

TEST(Hash, testByteArrayConstructor) {
  EXPECT_EQ(testHashHex, testHash.toString());
}

TEST(Hash, testByteRangeConstructor) {
  unsigned char bytes[] = {
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
  Hash hash(folly::ByteRange(bytes, Hash::RAW_SIZE));
  EXPECT_EQ(hash, testHash);
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

TEST(Hash, ensureStringConstructorRejectsArgumentWithWrongLength) {
  EXPECT_THROW(Hash("badfood"), std::invalid_argument);
}

TEST(Hash, ensureStringConstructorRejectsArgumentBadCharacters) {
  EXPECT_THROW(
      Hash("ZZZZb00cdeadbeefc00010ff1badb0028badf00d"), std::invalid_argument);
}
