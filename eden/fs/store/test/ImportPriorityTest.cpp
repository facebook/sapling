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

#include "eden/fs/utils/StaticAssert.h"

namespace {

using namespace facebook::eden;

static_assert(CheckSize<ImportPriority, sizeof(uint64_t)>());

TEST(ImportPriorityTest, basic_class_comparison) {
  EXPECT_LT(
      ImportPriority{ImportPriority::Class::Normal},
      ImportPriority{ImportPriority::Class::High});
  EXPECT_LT(
      ImportPriority{ImportPriority::Class::Low},
      ImportPriority{ImportPriority::Class::Normal});
}

TEST(ImportPriorityTest, deprioritized_keeps_class_but_compares_lower) {
  auto initial = ImportPriority{};
  auto lower = initial.adjusted(-1);
  EXPECT_EQ(initial.getClass(), lower.getClass());
  EXPECT_LT(lower, initial);
}

TEST(ImportPriorityTest, format) {
  EXPECT_EQ(
      "(Normal, +0)",
      fmt::to_string(ImportPriority{ImportPriority::Class::Normal}));
  EXPECT_EQ(
      "(High, -10)",
      fmt::to_string(ImportPriority{ImportPriority::Class::High, -10}));
  EXPECT_EQ(
      "(Low, +10)",
      fmt::to_string(ImportPriority{ImportPriority::Class::Low, 10}));
}

TEST(ImportPriorityTest, minimum_value_cannot_be_deprioritized) {
  auto minimum = ImportPriority::minimumValue();
  EXPECT_EQ(minimum, minimum.adjusted(-1));
}

} // namespace
