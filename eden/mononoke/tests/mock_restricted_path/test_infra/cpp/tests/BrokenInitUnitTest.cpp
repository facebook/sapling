// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <gtest/gtest.h>

class BadEnvironment : public ::testing::Environment {
  void SetUp() override {
    if (std::getenv("TPX_PLAYGROUND_FAIL")) {
      GTEST_FAIL();
    }
  }
  void TearDown() override {}
};

void* env = ::testing::AddGlobalTestEnvironment(new BadEnvironment());

TEST(EnvironmentsAreWeird, right) {
  EXPECT_TRUE(true) << "OK!";
}
