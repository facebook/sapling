/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/digest/Blake3.h"

#include <string>

#include <folly/Range.h>
#include <folly/String.h>
#include <gtest/gtest.h>

using namespace facebook::eden;
using folly::ByteRange;
using folly::StringPiece;
using std::string;

namespace {

constexpr folly::StringPiece kData = "Hello, World!";
constexpr folly::StringPiece kKey = "19700101-1111111111111111111111#";

TEST(Blake3, blake3Test) {
  Blake3 blake3;
  blake3.update(kData);
  std::array<uint8_t, 32> out;
  blake3.finalize(folly::MutableByteRange(out.data(), out.size()));

  EXPECT_EQ(
      folly::hexlify(folly::ByteRange(out.data(), out.size())),
      "288a86a79f20a3d6dccdca7713beaed178798296bdfa7913fa2a62d9727bf8f8");
}

TEST(Blake3, keyedBlake3Test) {
  const folly::ByteRange key(kKey);
  Blake3 blake3(key);
  blake3.update(kData);
  std::array<uint8_t, 32> out;
  blake3.finalize(folly::MutableByteRange(out.data(), out.size()));

  EXPECT_EQ(
      folly::hexlify(folly::ByteRange(out.data(), out.size())),
      "762a2729ed3c2c1b5ec9523761e43bf215589dc8f1844a11a6a987f19cfab0e0");
}

TEST(Blake3, blake3EmptyTest) {
  Blake3 blake3;
  std::array<uint8_t, 32> out;
  blake3.finalize(folly::MutableByteRange(out.data(), out.size()));

  EXPECT_EQ(
      folly::hexlify(folly::ByteRange(out.data(), out.size())),
      "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
}

TEST(Blake3, emptyKeyedBlake3Test) {
  const folly::ByteRange key(kKey);
  Blake3 blake3(key);
  std::array<uint8_t, 32> out;
  blake3.finalize(folly::MutableByteRange(out.data(), out.size()));

  EXPECT_EQ(
      folly::hexlify(folly::ByteRange(out.data(), out.size())),
      "e898b912a31fc35d7b3522173f5e8549ea08e3e8edd9b0586a3344d07d6d85f3");
}
} // namespace
