/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/win/utils/StringConv.h"
#include <string>
#include "gtest/gtest.h"

using namespace facebook::eden;

TEST(StringConvTest, multibyteToWideString) {
  EXPECT_EQ(L"", multibyteToWideString(""));
  EXPECT_EQ(L"foobar", multibyteToWideString("foobar"));
  EXPECT_EQ(
      L"\u0138\u00F9\u0150\U00029136",
      multibyteToWideString(u8"\u0138\u00F9\u0150\U00029136"));
}

TEST(StringConvTest, wideToMultibyteString) {
  EXPECT_EQ(wideToMultibyteString<std::string>(L""), "");
  EXPECT_EQ(wideToMultibyteString<std::string>(L"foobar"), "foobar");
  EXPECT_EQ(
      wideToMultibyteString<std::string>(L"\u0138\u00F9\u0150\U00029136"),
      u8"\u0138\u00F9\u0150\U00029136");
}
