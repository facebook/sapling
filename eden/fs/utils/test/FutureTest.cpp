/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Future.h"
#include <folly/portability/GTest.h>

using namespace facebook::eden;
using namespace folly;

TEST(FutureTest, collectSafe_completes_when_all_futures_do) {
  Promise<int> p1;
  Promise<int> p2;
  auto result = collectSafe(p1.getFuture(), p2.getFuture());
  EXPECT_FALSE(result.isReady());
  p2.setValue(10);
  EXPECT_FALSE(result.isReady());
  p1.setValue(5);
  EXPECT_TRUE(result.isReady());
  EXPECT_EQ(std::make_tuple(5, 10), result.value());
}

TEST(FutureTest, collectSafe_completes_after_last_exception_with_first_error) {
  Promise<int> p1;
  Promise<int> p2;
  auto result = collectSafe(p1.getFuture(), p2.getFuture());
  EXPECT_FALSE(result.isReady());
  p2.setException(std::runtime_error{"one"});
  EXPECT_FALSE(result.isReady());
  p1.setException(std::runtime_error{"two"});
  EXPECT_TRUE(result.isReady());
  EXPECT_EQ(
      std::string{"one"},
      result.result().exception().get_exception<std::runtime_error>()->what());
}
