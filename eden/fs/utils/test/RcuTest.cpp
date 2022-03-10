/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Rcu.h"
#include <folly/portability/GTest.h>
#include <memory>

using namespace facebook::eden;

TEST(RcuTest, rlock) {
  RcuPtr<int> rcu{folly::rcu_default_domain(), 42};
  auto guard = rcu.rlock();
  EXPECT_EQ(*guard, 42);
}

TEST(RcuTest, update) {
  RcuPtr<int> rcu{folly::rcu_default_domain(), 42};
  auto guard = rcu.rlock();
  rcu.update(10);
  EXPECT_EQ(*guard, 42);

  auto guard2 = rcu.rlock();
  EXPECT_EQ(*guard2, 10);
}

TEST(RcuTest, exchange) {
  RcuPtr<int> rcu{folly::rcu_default_domain(), 42};
  auto guard = rcu.rlock();
  auto old = rcu.exchange(10);
  EXPECT_EQ(*old, 42);
  EXPECT_EQ(*guard, *old);
  // Silence LeakSanitizer, do not manually delete the pointer without calling
  // synchronize first.
  delete old;
}

TEST(RcuTest, synchronize) {
  RcuPtr<int> rcu{folly::rcu_default_domain(), 42};
  rcu.synchronize();
  auto guard = rcu.rlock();
  EXPECT_EQ(*guard, 42);
}

bool updateAndSynchronizeDeleted = false;
TEST(RcuTest, updateAndSynchronize) {
  struct Deleter {
    void operator()(int* value) {
      delete value;
      updateAndSynchronizeDeleted = true;
    }
  };

  RcuPtr<int, folly::RcuTag, Deleter> rcu{folly::rcu_default_domain(), 42};
  rcu.update(10);
  rcu.synchronize();
  EXPECT_TRUE(updateAndSynchronizeDeleted);
}

int updateAndSynchronizeDeletedCount = 0;
TEST(RcuUniquePtr, updateAndSynchronize) {
  struct Deleter {
    void operator()(int* value) {
      delete value;
      updateAndSynchronizeDeletedCount++;
    }
  };

  RcuPtr<int, folly::RcuTag, Deleter> rcu{folly::rcu_default_domain()};
  rcu.update(std::unique_ptr<int, Deleter>{new int(42)});
  rcu.update(std::unique_ptr<int, Deleter>{new int(43)});
  rcu.synchronize();
  EXPECT_EQ(updateAndSynchronizeDeletedCount, 1);

  rcu.reset();
  rcu.synchronize();
  EXPECT_EQ(updateAndSynchronizeDeletedCount, 2);
}
