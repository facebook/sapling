/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>
#include <cstdio>

#include "eden/scm/lib/backingstore/c_api/RustBackingStore.h"

TEST(CFallible, returns_ok) {
  RustCFallible<uint8_t> result(
      rust_test_cfallible_ok(), rust_test_cfallible_ok_free);

  uint8_t abc = *result.get();

  EXPECT_EQ(abc, 0xFB);
  EXPECT_EQ(result.isError(), false);
}

// Test case for correct memory management when value is not used.
TEST(CFallible, returns_ok_no_consume) {
  RustCFallible<uint8_t> result(
      rust_test_cfallible_ok(), rust_test_cfallible_ok_free);
  EXPECT_EQ(result.isError(), false);
}

TEST(CFallible, returns_err) {
  RustCFallible<uint8_t> result(
      rust_test_cfallible_err(), rust_test_cfallible_ok_free);

  EXPECT_EQ(result.get(), nullptr);
  EXPECT_EQ(result.isError(), true);
  EXPECT_STREQ(result.getError(), "failure!");
}
