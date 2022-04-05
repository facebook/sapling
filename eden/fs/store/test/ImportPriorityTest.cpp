/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/ImportPriority.h"

#include <folly/logging/xlog.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>

using namespace facebook::eden;

TEST(ImportPriorityTest, value) {
  EXPECT_LT(ImportPriority::kNormal(), ImportPriority::kHigh());
  EXPECT_LT(ImportPriority::kLow(), ImportPriority::kNormal());

  // The maximum possible priority
  auto maximum = ImportPriority{ImportPriorityKind::High, 0xFFFFFFFFFFFFFFF};
  EXPECT_EQ(maximum.value(), 0x2FFFFFFFFFFFFFFF);

  // the minimum possible priority
  auto minimum = ImportPriority{ImportPriorityKind::Low, 0};
  EXPECT_EQ(minimum.value(), 0x0000000000000000);
}
