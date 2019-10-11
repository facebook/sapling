/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/PathMap.h"
#include <gtest/gtest.h>

using facebook::eden::PathComponent;
using facebook::eden::PathComponentPiece;
using facebook::eden::PathMap;
using namespace facebook::eden::path_literals;

TEST(PathMap, insert) {
  PathMap<bool> map;

  EXPECT_TRUE(map.empty());

  map.insert(std::make_pair(PathComponent("foo"), true));
  EXPECT_EQ(1, map.size());
  EXPECT_NE(map.end(), map.find("foo"_pc));
  EXPECT_TRUE(map.at("foo"_pc));
  EXPECT_TRUE(map["foo"_pc]);

  // operator[] creates an entry for missing key
  map["bar"_pc] = false;
  EXPECT_EQ(2, map.size());
  EXPECT_NE(map.end(), map.find("bar"_pc));
  EXPECT_FALSE(map.at("bar"_pc));
  EXPECT_FALSE(map["bar"_pc]);

  // at() throws for missing key
  EXPECT_THROW(map.at("notpresent"_pc), std::out_of_range);

  // Test the const version of find(), at() and operator[]
  const PathMap<bool>& cmap = map;
  EXPECT_NE(cmap.cend(), cmap.find("bar"_pc));
  EXPECT_FALSE(cmap.at("bar"_pc));
  EXPECT_FALSE(cmap["bar"_pc]);

  // const operator[] throws for missing key
  EXPECT_THROW(cmap["notpresent"_pc], std::out_of_range);
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
      "bar"_pc,
      "baz"_pc,
      "foo"_pc,
  };
  EXPECT_EQ(expect, keys);

  auto iter = map.find("baz"_pc);
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

  auto result = map.emplace("one"_pc, true, 42);
  EXPECT_EQ(1, EmplaceTest::counter)
      << "construct a single EmplaceTest instance";
  EXPECT_NE(map.end(), result.first);
  EXPECT_TRUE(result.second) << "inserted";
  EXPECT_TRUE(map.at("one"_pc).dummy);

  // Second emplace with the same key has no effect
  result = map.emplace("one"_pc, false, 42);
  EXPECT_EQ(1, EmplaceTest::counter)
      << "did not construct another EmplaceTest instance";
  EXPECT_FALSE(result.second) << "did not insert";
  EXPECT_TRUE(map.at("one"_pc).dummy) << "didn't change value to false";
}

TEST(PathMap, swap) {
  PathMap<std::string> b, a{std::make_pair(PathComponent("foo"), "foo")};

  b.swap(a);
  EXPECT_EQ(0, a.size()) << "a now has 0 elements";
  EXPECT_EQ(1, b.size()) << "b now has 1 element";
  EXPECT_EQ("foo", b.at("foo"_pc));

  a = std::move(b);
  EXPECT_EQ(1, a.size()) << "a now has 1 element";
  EXPECT_EQ(0, b.size()) << "b now has 0 elements";
  EXPECT_EQ("foo", a.at("foo"_pc));
}
