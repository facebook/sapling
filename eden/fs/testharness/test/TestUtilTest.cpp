/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/TestUtil.h"

#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/model/ObjectId.h"
#include "eden/fs/model/TestOps.h"

using namespace facebook::eden;

TEST(TestUtil, makeTestId) {
  EXPECT_EQ(
      ObjectId::fromHex("0000000000000000000000000000000000000001"),
      makeTestId("1"));
  EXPECT_EQ(
      ObjectId::fromHex("0000000000000000000000000000000000000022"),
      makeTestId("22"));
  EXPECT_EQ(
      ObjectId::fromHex("0000000000000000000000000000000000000abc"),
      makeTestId("abc"));
  EXPECT_EQ(
      ObjectId::fromHex("123456789abcdef0fedcba9876543210faceb00c"),
      makeTestId("123456789abcdef0fedcba9876543210faceb00c"));
  EXPECT_EQ(
      ObjectId::fromHex("0000000000000000000000000000000000000000"),
      makeTestId(""));
  EXPECT_THROW_RE(
      makeTestId("123456789abcdef0fedcba9876543210faceb00c1"),
      std::invalid_argument,
      "too big");
  EXPECT_THROW_RE(makeTestId("z"), std::exception, "invalid hex digit");
  EXPECT_THROW_RE(
      // There's a "g" in the string below
      makeTestId("123456789abcdefgfedcba9876543210faceb00c"),
      std::exception,
      "invalid hex digit");
}
