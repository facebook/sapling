/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/utils/LazyInitialize.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

template <typename T>
using SynchronizedSharedPtr = folly::Synchronized<std::shared_ptr<T>>;

auto unimplemented = []() -> std::shared_ptr<std::string> {
  throw std::runtime_error("unimplemented!");
};

TEST(LazyInitializeTest, returnValue) {
  SynchronizedSharedPtr<std::string> ptr(
      std::make_shared<std::string>("hello"));
  auto result = lazyInitialize(true, ptr, unimplemented);

  EXPECT_EQ(result->compare("hello"), 0);
}

TEST(LazyInitializeTest, returnNull) {
  SynchronizedSharedPtr<std::string> ptr(nullptr);

  auto result = lazyInitialize(false, ptr, unimplemented);

  EXPECT_EQ(result, nullptr);
}

TEST(LazyInitializeTest, initialize) {
  SynchronizedSharedPtr<std::string> ptr(nullptr);

  auto result = lazyInitialize(
      true, ptr, []() { return std::make_shared<std::string>("called"); });

  EXPECT_EQ(result->compare("called"), 0);

  // another check to make sure it won't initialize twice
  lazyInitialize(true, ptr, unimplemented);
}

TEST(LazyInitializeTest, deletePtr) {
  SynchronizedSharedPtr<std::string> ptr(
      std::make_shared<std::string>("hello"));
  auto result = lazyInitialize(false, ptr, unimplemented);

  EXPECT_EQ(result, nullptr);
  EXPECT_EQ(*ptr.rlock(), nullptr);
}
