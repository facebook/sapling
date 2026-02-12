// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

#include <boost/filesystem.hpp>
#include <gtest/gtest.h>
#include <stdlib.h>
#include <chrono>
#include <iostream>
#include <string>
#include <thread>

TEST(SimpleTest, str_eq) {
  if (const char* splay = std::getenv("TPX_PLAYGROUND_SPLAY")) {
    // This sleep is intentional, to make it easy to see how the test behaves
    // when the labels "serialize" and "serialize_test_cases" are present.
    std::this_thread::sleep_for(
        std::chrono::milliseconds(std::stoi(splay))); // NOLINT
  }
  EXPECT_STREQ("testing", "testing");
}

TEST(SimpleTest, str_neq) {
  if (const char* splay = std::getenv("TPX_PLAYGROUND_SPLAY")) {
    // This sleep is intentional, to make it easy to see how the test behaves
    // when the labels "serialize" and "serialize_test_cases" are present.
    std::this_thread::sleep_for(
        std::chrono::milliseconds(std::stoi(splay))); // NOLINT
  }
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
  if (std::getenv("TPX_PLAYGROUND_LEAK")) {
    // Generate a memory leak to trigger LeakSanitizer.
    int* leak = (int*)malloc(sizeof(int));
    *leak = 42;
    EXPECT_EQ(*leak, 42);
  }
  // just assert and pass
  std::cout << "normal branch\n";
  EXPECT_STREQ("testing", "testing");
}

TEST(SimpleTest, test_name) {
  if (const char* splay = std::getenv("TPX_PLAYGROUND_SPLAY")) {
    // This sleep is intentional, to make it easy to see how the test behaves
    // when the labels "serialize" and "serialize_test_cases" are present.
    std::this_thread::sleep_for(
        std::chrono::milliseconds(std::stoi(splay))); // NOLINT
  }
  EXPECT_EQ(42, 42);
}

// Enable sanity checking that we only run "test_name" by itself.
TEST(SimpleTest, test_name_with_other_test_as_prefix) {
  if (const char* splay = std::getenv("TPX_PLAYGROUND_SPLAY")) {
    // This sleep is intentional, to make it easy to see how the test behaves
    // when the labels "serialize" and "serialize_test_cases" are present.
    std::this_thread::sleep_for(
        std::chrono::milliseconds(std::stoi(splay))); // NOLINT
  }
  EXPECT_EQ(42, 42);
}

TEST(SimpleTest, test_execution_env_should_be_set) {
  if (const char* splay = std::getenv("TPX_PLAYGROUND_SPLAY")) {
    // This sleep is intentional, to make it easy to see how the test behaves
    // when the labels "serialize" and "serialize_test_cases" are present.
    std::this_thread::sleep_for(
        std::chrono::milliseconds(std::stoi(splay))); // NOLINT
  }
  EXPECT_NE(std::getenv("TPX_IS_TEST_EXECUTION"), nullptr);
}
