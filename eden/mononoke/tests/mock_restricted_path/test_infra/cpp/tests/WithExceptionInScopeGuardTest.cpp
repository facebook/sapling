// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <folly/ScopeGuard.h>
#include <gtest/gtest.h>
#include <iostream>
#include <string>

TEST(SimpleTest, str_eq) {
  EXPECT_STREQ("testing", "testing");
}

TEST(SimpleTest, str_neq) {
  EXPECT_STREQ("not_testing", "not_testing");
}

TEST(SimpleTest, playground_test) {
  std::cout << "playground stdout\n";
  std::cerr << "playground stderr\n";

  // Throwing an exception during scope exit currently leads to a FATAL
  SCOPE_EXIT {
    if (std::getenv("TPX_PLAYGROUND_FATAL")) {
      std::cerr << "fatal branch\n";
      throw std::runtime_error("Fatal error");
    }
  };

  if (std::getenv("TPX_PLAYGROUND_FAIL")) {
    std::cerr << "fail branch\n";
    EXPECT_STREQ("testing", "nope");
  } else if (std::getenv("TPX_PLAYGROUND_SKIP")) {
    GTEST_SKIP();
  } else { // just assert and pass
    std::cout << "normal branch\n";
    EXPECT_STREQ("testing", "testing");
  }
}

TEST(SimpleTest, playground_test2) {
  EXPECT_EQ(42, 42);
}
