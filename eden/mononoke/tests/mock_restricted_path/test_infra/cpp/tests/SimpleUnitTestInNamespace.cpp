// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <boost/filesystem.hpp>
#include <gtest/gtest.h>
#include <chrono>
#include <iostream>
#include <string>
#include <thread>

namespace facebook::testinfra {

TEST(SimpleTest, str_eq) {
  EXPECT_STREQ("testing", "testing");
}

TEST(SimpleTest, str_neq) {
  EXPECT_STREQ("not_testing", "not_testing");
}

TEST(SimpleTest, playground_test) {
  std::cout << "playground stdout\n";
  std::cerr << "playground stderr\n";

  if (std::getenv("TPX_PLAYGROUND_FAIL")) {
    std::cerr << "fail branch\n";
    EXPECT_STREQ("testing", "nope");
  } else if (std::getenv("TPX_PLAYGROUND_FATAL")) {
    std::cerr << "fatal branch\n";
    char* c = nullptr;
    std::cout << *c;
  } else if (std::getenv("TPX_PLAYGROUND_SKIP")) {
    GTEST_SKIP();
  } else if (std::getenv("TPX_PLAYGROUND_SLEEP")) {
    int i = std::stoi(std::getenv("TPX_PLAYGROUND_SLEEP"));
    auto duration = std::chrono::seconds(i);
    // This sleep is intentional, we want to test target to timeout on
    // request.
    //
    // NOLINTNEXTLINE(facebook-hte-BadCall-sleep_for)
    std::this_thread::sleep_for(duration);
  }
  // just assert and pass
  std::cout << "normal branch\n";
  EXPECT_STREQ("testing", "testing");
}

TEST(SimpleTest, test_name) {
  EXPECT_EQ(42, 42);
}

// Enable sanity checking that we only run "test_name" by itself.
TEST(SimpleTest, test_name_with_other_test_as_prefix) {
  EXPECT_EQ(42, 42);
}

} // namespace facebook::testinfra
