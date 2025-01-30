/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/RingBuffer.h"
#include <gmock/gmock.h>
#include <gtest/gtest.h>

namespace {

using testing::ElementsAre;
using namespace facebook::eden;

TEST(RingBufferTest, starts_empty) {
  RingBuffer<int> b{4};
  EXPECT_EQ(0, b.size());
}

TEST(RingBufferTest, elements_can_be_retrieved_as_vector) {
  RingBuffer<int> b{4};
  b.push(1);
  b.push(2);

  EXPECT_THAT(b.toVector(), ElementsAre(1, 2));
}

TEST(RingBufferTest, exact_size) {
  RingBuffer<int> b{4};
  b.push(1);
  b.push(2);
  b.push(3);
  b.push(4);

  EXPECT_THAT(b.toVector(), ElementsAre(1, 2, 3, 4));
}

TEST(RingBufferTest, wraps_around) {
  RingBuffer<int> b{4};
  b.push(1);
  b.push(2);
  b.push(3);
  b.push(4);
  b.push(5);
  b.push(6);

  EXPECT_THAT(b.toVector(), ElementsAre(3, 4, 5, 6));
}

TEST(RingBufferTest, insert_non_temporary) {
  RingBuffer<int> b{4};
  int x = 10;
  b.push(x);
  EXPECT_EQ(1, b.toVector().size());
}

TEST(RingBufferTest, zero_size) {
  RingBuffer<int> b{0};
  b.push(1);
  b.push(2);
  b.push(3);
  EXPECT_EQ(0, b.toVector().size());
}

TEST(RingBufferTest, extract) {
  RingBuffer<int> b{4};
  b.push(1);
  b.push(2);
  b.push(3);
  b.push(4);
  b.push(5);
  b.push(6);
  b.push(7);

  auto v = std::move(b).extractVector();

  EXPECT_EQ(4, v.size());
  EXPECT_NE(v.end(), std::find(v.begin(), v.end(), 4));
  EXPECT_NE(v.end(), std::find(v.begin(), v.end(), 5));
  EXPECT_NE(v.end(), std::find(v.begin(), v.end(), 6));
  EXPECT_NE(v.end(), std::find(v.begin(), v.end(), 7));
}

} // namespace
