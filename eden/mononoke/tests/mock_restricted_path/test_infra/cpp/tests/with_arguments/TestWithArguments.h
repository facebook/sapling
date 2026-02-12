// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#pragma once
#include <string>
#include "gtest/gtest.h"

namespace {
std::string g_command_line_arg;
}

class MyTestEnvironment : public testing::Environment {
 public:
  explicit MyTestEnvironment(const std::string& command_line_arg) {
    g_command_line_arg = command_line_arg;
  }
};

TEST(MyTest, command_line_arg_test) {
  ASSERT_EQ(g_command_line_arg, "hello world");
}
