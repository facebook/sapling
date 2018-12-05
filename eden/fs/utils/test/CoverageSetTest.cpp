/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "eden/fs/utils/CoverageSet.h"
#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(CoverageSetTest, empty_ranges_are_always_covered) {
  CoverageSet s;
  EXPECT_TRUE(s.covers(0, 0));
  EXPECT_TRUE(s.covers(1, 1));
}

TEST(CoverageSetTest, tracks_ranges) {
  CoverageSet s;
  EXPECT_FALSE(s.covers(0, 1));
  EXPECT_FALSE(s.covers(0, 2));
  EXPECT_FALSE(s.covers(1, 2));

  s.add(0, 1);
  s.add(0, 2);
  EXPECT_TRUE(s.covers(0, 2));
  EXPECT_FALSE(s.covers(0, 5));

  s.add(3, 5);
  EXPECT_TRUE(s.covers(3, 5));
  EXPECT_TRUE(s.covers(3, 4));
  EXPECT_FALSE(s.covers(3, 6));
  EXPECT_FALSE(s.covers(0, 5));

  s.add(2, 3);
  EXPECT_TRUE(s.covers(0, 3));
  EXPECT_TRUE(s.covers(3, 5));
  EXPECT_TRUE(s.covers(0, 4));
  EXPECT_TRUE(s.covers(0, 5));
  EXPECT_FALSE(s.covers(0, 6));
}

TEST(CoverageSetTest, sequential_ranges_merge) {
  CoverageSet s;
  EXPECT_EQ(0, s.getIntervalCount());
  s.add(0, 10);
  EXPECT_EQ(1, s.getIntervalCount());
  s.add(10, 20);
  EXPECT_EQ(1, s.getIntervalCount());
  s.add(20, 30);
  EXPECT_EQ(1, s.getIntervalCount());
  s.add(30, 40);
  EXPECT_EQ(1, s.getIntervalCount());
  EXPECT_TRUE(s.covers(0, 40));
}

TEST(CoverageSetTest, merges_ranges_on_both_sides) {
  CoverageSet s;
  s.add(0, 2);
  s.add(3, 5);
  EXPECT_EQ(2, s.getIntervalCount());
  s.add(2, 3);
  EXPECT_EQ(1, s.getIntervalCount());
  EXPECT_TRUE(s.covers(0, 5));
}

TEST(CoverageSetTest, merge_can_replace_many_nodes) {
  CoverageSet s;
  s.add(1, 2);
  s.add(3, 4);
  s.add(5, 6);
  s.add(7, 8);
  EXPECT_EQ(4, s.getIntervalCount());
  s.add(2, 7);
  EXPECT_EQ(1, s.getIntervalCount());

  EXPECT_FALSE(s.covers(0, 2));
  EXPECT_FALSE(s.covers(7, 9));
  EXPECT_TRUE(s.covers(1, 8));
}
