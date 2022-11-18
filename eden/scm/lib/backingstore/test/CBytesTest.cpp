/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Range.h>
#include <gtest/gtest.h>
#include <cstring>

#include "eden/scm/lib/backingstore/c_api/BackingStoreBindings.h"

namespace {

using namespace sapling;

TEST(CBytes, returns_hello_world) {
  auto result = sapling_test_cbytes();
  folly::ByteRange expected = folly::StringPiece("hello world");
  auto resultBytes = result.asByteRange();

  EXPECT_EQ(resultBytes, expected);
}

} // namespace
