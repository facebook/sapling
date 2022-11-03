/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/SourceLocation.h"

#include <fmt/core.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>

namespace {

using namespace facebook::eden;

TEST(SourceLocation, line_numbers_are_different) {
  auto a = EDEN_CURRENT_SOURCE_LOCATION;
  auto b = EDEN_CURRENT_SOURCE_LOCATION;
  EXPECT_EQ(a.function_name(), b.function_name());
  EXPECT_EQ(a.file_name(), b.file_name());
  EXPECT_NE(a.line(), b.line());

  fmt::print("function_name = {}\n", a.function_name());
  fmt::print("file_name = {}\n", a.file_name());
}

SourceLocation foo() {
  return EDEN_CURRENT_SOURCE_LOCATION;
}

TEST(SourceLocation, contains_function_name) {
  EXPECT_THAT(foo().function_name(), testing::HasSubstr("foo"));
}

TEST(SourceLocation, copy) {
  auto a = EDEN_CURRENT_SOURCE_LOCATION;
  auto b = a;
  b = a;
  EXPECT_EQ(a.line(), b.line());
}

} // namespace
