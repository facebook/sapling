/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/testharness/TestUtil.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/model/Hash.h"

using namespace facebook::eden;

TEST(TestUtil, makeTestHash) {
  EXPECT_EQ(
      Hash{"0000000000000000000000000000000000000001"}, makeTestHash("1"));
  EXPECT_EQ(
      Hash{"0000000000000000000000000000000000000022"}, makeTestHash("22"));
  EXPECT_EQ(
      Hash{"0000000000000000000000000000000000000abc"}, makeTestHash("abc"));
  EXPECT_EQ(
      Hash{"123456789abcdef0fedcba9876543210faceb00c"},
      makeTestHash("123456789abcdef0fedcba9876543210faceb00c"));
  EXPECT_EQ(Hash{"0000000000000000000000000000000000000000"}, makeTestHash(""));
  EXPECT_THROW_RE(
      makeTestHash("123456789abcdef0fedcba9876543210faceb00c1"),
      std::invalid_argument,
      "too big");
  EXPECT_THROW_RE(makeTestHash("z"), std::exception, "invalid hex digit");
  EXPECT_THROW_RE(
      // There's a "g" in the string below
      makeTestHash("123456789abcdefgfedcba9876543210faceb00c"),
      std::exception,
      "invalid hex digit");
}
