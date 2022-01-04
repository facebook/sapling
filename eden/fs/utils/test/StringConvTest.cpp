/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifdef _WIN32

#include "eden/fs/utils/StringConv.h"
#include <folly/portability/GTest.h>
#include <string>

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

#endif
