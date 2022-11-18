/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>
#include <cstdio>

#include "eden/scm/lib/backingstore/c_api/RustBackingStore.h"

namespace {

using namespace sapling;

TEST(CFallible, returns_ok) {
  CFallible<uint8_t, sapling_test_cfallible_ok_free> result{
      sapling_test_cfallible_ok()};

  uint8_t abc = *result.get();

  EXPECT_EQ(abc, 0xFB);
  EXPECT_EQ(result.isError(), false);
}

// Test case for correct memory management when value is not used.
TEST(CFallible, returns_ok_no_consume) {
  CFallible<uint8_t, sapling_test_cfallible_ok_free> result{
      sapling_test_cfallible_ok()};
  EXPECT_EQ(result.isError(), false);
}

TEST(CFallible, returns_err) {
  CFallible<uint8_t, sapling_test_cfallible_ok_free> result{
      sapling_test_cfallible_err()};

  EXPECT_EQ(result.get(), nullptr);
  EXPECT_EQ(result.isError(), true);
  EXPECT_STREQ(result.getError(), "context\n\nCaused by:\n    failure!");
}

} // namespace
