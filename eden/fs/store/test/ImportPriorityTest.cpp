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
  EXPECT_LT(
      ImportPriority{static_cast<ImportPriorityKind>(INT16_MIN)},
      ImportPriority::kNormal());
  EXPECT_LT(
      ImportPriority::kHigh(),
      ImportPriority{static_cast<ImportPriorityKind>(INT16_MAX)});

  // The maximum possible priority
  auto maximum = ImportPriority{
      static_cast<ImportPriorityKind>(INT16_MAX), 0xFFFFFFFFFFFF};
  EXPECT_EQ(maximum.value(), 0x7FFFFFFFFFFFFFFF);

  // the minimum possible priority
  auto minimum = ImportPriority{static_cast<ImportPriorityKind>(INT16_MIN), 0};
  EXPECT_EQ(minimum.value(), -0x8000000000000000);
}
