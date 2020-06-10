/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Utf8.h"
#include <gtest/gtest.h>

using namespace facebook::eden;

namespace {
constexpr folly::StringPiece kValidStrings[] = {
    "",
    "abcdef",
    "\0foo\n\0",
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

  EXPECT_EQ(u8"\uFFFD", ensureValidUtf8("\xff"));
  // overlong
  EXPECT_EQ(
      u8"foo\uFFFD\uFFFD\uFFFD\uFFFDbar",
      ensureValidUtf8("foo\xF0\x82\x82\xAC"
                      "bar"));
  EXPECT_EQ(u8"\uFFFDprefix\uFFFD", ensureValidUtf8("\xA0prefix\xB0"));
}
