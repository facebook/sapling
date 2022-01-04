/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/config/TomlConfig.h"
#include <folly/portability/GTest.h>

using namespace facebook::eden;
using namespace std::literals;

TEST(TomlConfigTest, setDefault_creates_tables_as_necessary) {
  auto table = cpptoml::make_table();
  auto [value, inserted] = setDefault(*table, {"foo", "bar", "baz"}, "value"s);
  EXPECT_TRUE(inserted);
  EXPECT_EQ("value", value);
}

TEST(TomlConfigTest, setDefault_returns_existing_value) {
  auto table = cpptoml::make_table();
  setDefault(*table, {"foo", "bar", "baz"}, "one"s);
  auto [value, inserted] = setDefault(*table, {"foo", "bar", "baz"}, "two"s);
  EXPECT_FALSE(inserted);
  EXPECT_EQ("one", value);
}

TEST(TomlConfigTest, throws_if_path_traverses_non_table) {
  auto table = cpptoml::make_table();
  setDefault(*table, {"foo", "bar"}, "string value"s);

  try {
    setDefault(*table, {"foo", "bar", "baz"}, "deeper value"s);
    FAIL();
  } catch (const std::exception& e) {
    EXPECT_EQ("foo.bar is not a table"s, e.what());
  }
}

TEST(TomlConfigTest, throws_if_existing_value_has_wrong_type) {
  auto table = cpptoml::make_table();
  setDefault(*table, {"foo", "bar"}, int64_t{1234});

  try {
    setDefault(*table, {"foo", "bar"}, "string value"s);
    FAIL();
  } catch (const std::exception& e) {
    EXPECT_EQ("foo.bar has mismatched type"s, e.what());
  }
}
