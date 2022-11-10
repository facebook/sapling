/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/String.h"

#include <folly/portability/GTest.h>

namespace facebook::eden {
namespace {

// It's okay in the future if we kill `eden::string_view` and consistently use
// `std::string_view` everywhere. In the meantime, however, to avoid
// platform-specific compiler errors, it makes sense to ensure they're different
// types.
static_assert(!std::is_same_v<std::string_view, string_view>);

struct TestCase {
  std::string_view haystack;
  std::string_view needle;
  bool result;
};

const TestCase startsWithTests[] = {
    {"haystack", "hay", true},
    {"haystack", "ay", false},
    {"haystack", "", true},
    {"", "", true},
    {"", "x", false},
    {"haystack", "haystackhaystack", false},
};

TEST(String, starts_with) {
  for (auto& tc : startsWithTests) {
    EXPECT_EQ(tc.result, starts_with(tc.haystack, tc.needle))
        << "starts_with: haystack=" << tc.haystack << " needle=" << tc.needle;
    EXPECT_EQ(tc.result, string_view{tc.haystack}.starts_with(tc.needle))
        << "starts_with: haystack=" << tc.haystack << " needle=" << tc.needle;
  }

  EXPECT_TRUE(string_view{"haystack"}.starts_with('h'));
  EXPECT_FALSE(string_view{"haystack"}.starts_with('k'));
}

const TestCase endsWithTests[] = {
    {"haystack", "hay", false},
    {"haystack", "ack", true},
    {"haystack", "", true},
    {"", "", true},
    {"", "x", false},
    {"haystack", "haystackhaystack", false},
};

TEST(String, ends_with) {
  for (auto& tc : endsWithTests) {
    EXPECT_EQ(tc.result, ends_with(tc.haystack, tc.needle))
        << "ends_with: haystack=" << tc.haystack << " needle=" << tc.needle;
    EXPECT_EQ(tc.result, string_view{tc.haystack}.ends_with(tc.needle))
        << "ends_with: haystack=" << tc.haystack << " needle=" << tc.needle;
  }

  EXPECT_TRUE(string_view{"haystack"}.ends_with('k'));
  EXPECT_FALSE(string_view{"haystack"}.ends_with('h'));
}

} // namespace
} // namespace facebook::eden
