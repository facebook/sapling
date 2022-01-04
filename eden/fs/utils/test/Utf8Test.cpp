/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Utf8.h"
#include <folly/portability/GTest.h>

using namespace facebook::eden;

namespace {
const folly::StringPiece kValidStrings[] = {
    "",
    "abcdef",
    "\0foo\n\0",
    reinterpret_cast<const char*>(u8"\u0080"), // 2 bytes
    reinterpret_cast<const char*>(u8"\u00A2"), // 2 bytes
    reinterpret_cast<const char*>(u8"\u0800"), // 3 bytes
    reinterpret_cast<const char*>(u8"\u0939"), // 3 bytes
    reinterpret_cast<const char*>(u8"\U00010348"), // 4 bytes
    reinterpret_cast<const char*>(u8"\U00040000"), // 4 bytes
};
}

TEST(Utf8Test, isValidUtf8) {
  for (auto str : kValidStrings) {
    EXPECT_TRUE(isValidUtf8(str));
  }

  EXPECT_FALSE(isValidUtf8("\xff"));
  // overlong
  EXPECT_FALSE(isValidUtf8("\xF0\x82\x82\xAC"));
  EXPECT_FALSE(isValidUtf8("\xA0prefix\xB0"));
}

TEST(Utf8String, ensureValidUtf8) {
  for (auto str : kValidStrings) {
    EXPECT_EQ(str, ensureValidUtf8(str));
  }

  EXPECT_EQ(reinterpret_cast<const char*>(u8"\uFFFD"), ensureValidUtf8("\xff"));
  // overlong
  EXPECT_EQ(
      reinterpret_cast<const char*>(u8"foo\uFFFD\uFFFD\uFFFD\uFFFDbar"),
      ensureValidUtf8("foo\xF0\x82\x82\xAC"
                      "bar"));
  EXPECT_EQ(
      reinterpret_cast<const char*>(u8"\uFFFDprefix\uFFFD"),
      ensureValidUtf8("\xA0prefix\xB0"));
}
