/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/utils/PathMap.h"

using facebook::eden::PathMap;
using facebook::eden::PathComponent;
using facebook::eden::PathComponentPiece;

TEST(PathMap, insert) {
  PathMap<bool> map;

  EXPECT_TRUE(map.empty());

  map.insert(std::make_pair(PathComponent("foo"), true));
  EXPECT_EQ(1, map.size());
  EXPECT_NE(map.end(), map.find(PathComponentPiece("foo")));
  EXPECT_TRUE(map.at(PathComponentPiece("foo")));
  EXPECT_TRUE(map[PathComponentPiece("foo")]);

  // operator[] creates an entry for missing key
  map[PathComponentPiece("bar")] = false;
  EXPECT_EQ(2, map.size());
  EXPECT_NE(map.end(), map.find(PathComponentPiece("bar")));
  EXPECT_FALSE(map.at(PathComponentPiece("bar")));
  EXPECT_FALSE(map[PathComponentPiece("bar")]);

  // at() throws for missing key
  EXPECT_THROW(map.at(PathComponentPiece("notpresent")), std::out_of_range);

  // Test the const version of find(), at() and operator[]
  [](const PathMap<bool>& map) {
    EXPECT_NE(map.cend(), map.find(PathComponentPiece("bar")));
    EXPECT_FALSE(map.at(PathComponentPiece("bar")));
    EXPECT_FALSE(map[PathComponentPiece("bar")]);

    // const operator[] throws for missing key
    EXPECT_THROW(map[PathComponentPiece("notpresent")], std::out_of_range);
  }(map);
}

TEST(PathMap, iteration_and_erase) {
  PathMap<int> map{
      std::make_pair(PathComponent("foo"), 1),
      std::make_pair(PathComponent("bar"), 2),
      std::make_pair(PathComponent("baz"), 3),
  };

  std::vector<PathComponentPiece> keys;
  for (const auto& it : map) {
    keys.emplace_back(it.first);
  }

  // Keys have deterministic order
  std::vector<PathComponentPiece> expect{
      PathComponentPiece("bar"),
      PathComponentPiece("baz"),
      PathComponentPiece("foo"),
  };
  EXPECT_EQ(expect, keys);

  auto iter = map.find(PathComponentPiece("baz"));
  EXPECT_EQ(3, iter->second);

  iter = map.erase(iter);
  EXPECT_EQ(2, map.size()) << "deleted 1";
  EXPECT_EQ(PathComponent("foo"), iter->first) << "iter advanced to next item";
  EXPECT_EQ(1, iter->second);
}

TEST(PathMap, copy) {
  PathMap<int> map{
      std::make_pair(PathComponent("foo"), 1),
      std::make_pair(PathComponent("bar"), 2),
      std::make_pair(PathComponent("baz"), 3),
  };
  PathMap<int> other = map;
  EXPECT_EQ(3, other.size());
  EXPECT_EQ(map, other);
}

TEST(PathMap, move) {
  PathMap<int> map{
      std::make_pair(PathComponent("foo"), 1),
      std::make_pair(PathComponent("bar"), 2),
      std::make_pair(PathComponent("baz"), 3),
  };
  PathMap<int> other = std::move(map);
  EXPECT_EQ(3, other.size());
  EXPECT_EQ(0, map.size());
}

struct EmplaceTest {
  static int counter;
  bool dummy;

  // secondArg is present to validate that emplace is correctly
  // forwarding multiple arguments.
  EmplaceTest(bool value, int secondArg) : dummy(value) {
    ++counter;
    ++secondArg; // Suppress lint about secondArg being unused
  }
};
int EmplaceTest::counter = 0;

TEST(PathMap, emplace) {
  PathMap<EmplaceTest> map;

  auto result = map.emplace(PathComponentPiece("one"), true, 42);
  EXPECT_EQ(1, EmplaceTest::counter)
      << "construct a single EmplaceTest instance";
  EXPECT_NE(map.end(), result.first);
  EXPECT_TRUE(result.second) << "inserted";
  EXPECT_TRUE(map.at(PathComponentPiece("one")).dummy);

  // Second emplace with the same key has no effect
  result = map.emplace(PathComponentPiece("one"), false, 42);
  EXPECT_EQ(1, EmplaceTest::counter)
      << "did not construct another EmplaceTest instance";
  EXPECT_FALSE(result.second) << "did not insert";
  EXPECT_TRUE(map.at(PathComponentPiece("one")).dummy)
      << "didn't change value to false";
}

TEST(PathMap, swap) {
  PathMap<std::string> b, a{std::make_pair(PathComponent("foo"), "foo")};

  b.swap(a);
  EXPECT_EQ(0, a.size()) << "a now has 0 elements";
  EXPECT_EQ(1, b.size()) << "b now has 1 element";
  EXPECT_EQ("foo", b.at(PathComponentPiece("foo")));

  a = std::move(b);
  EXPECT_EQ(1, a.size()) << "a now has 1 element";
  EXPECT_EQ(0, b.size()) << "b now has 0 elements";
  EXPECT_EQ("foo", a.at(PathComponentPiece("foo")));
}
