/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/PathMap.h"
#include <folly/portability/GTest.h>
#include <folly/portability/Unistd.h>

using namespace facebook::eden;
using namespace facebook::eden::path_literals;

TEST(PathMap, caseSensitive) {
  // Explicitly a case sensitive map, regardless of the host OS
  PathMap<bool> map(CaseSensitivity::Sensitive);

  map.insert(std::make_pair(PathComponent("foo"), true));
  EXPECT_TRUE(map.at("foo"_pc));
  EXPECT_EQ(map.find("Foo"_pc), map.end());

  EXPECT_TRUE(map.insert(std::make_pair(PathComponent("FOO"), false)).second);
  EXPECT_EQ(map.size(), 2);
  EXPECT_TRUE(map.at("foo"_pc));
  EXPECT_FALSE(map.at("FOO"_pc));
  EXPECT_EQ(map.erase("FOO"_pc), 1);
  EXPECT_EQ(map.size(), 1);

  map["FOO"_pc] = true;
  map["Foo"_pc] = false;
  EXPECT_EQ(map.size(), 3);
}

TEST(PathMap, caseSensitiveCopyMove) {
  PathMap<bool> map(CaseSensitivity::Sensitive);
  map.insert(std::make_pair(PathComponent("foo"), true));

  PathMap<bool> copied(map);
  EXPECT_TRUE(copied.at("foo"_pc));
  EXPECT_EQ(copied.find("Foo"_pc), copied.end());

  PathMap<bool> copy_assign(CaseSensitivity::Insensitive);
  copy_assign = map;
  EXPECT_TRUE(copy_assign.at("foo"_pc));
  EXPECT_EQ(copy_assign.find("Foo"_pc), copy_assign.end());

  PathMap<bool> moved(std::move(map));
  EXPECT_TRUE(moved.at("foo"_pc));
  EXPECT_EQ(moved.find("Foo"_pc), moved.end());

  PathMap<bool> move_assign(CaseSensitivity::Insensitive);
  move_assign = std::move(moved);
  EXPECT_TRUE(move_assign.at("foo"_pc));
  EXPECT_EQ(move_assign.find("Foo"_pc), move_assign.end());
}

TEST(PathMap, caseInSensitive) {
  // Explicitly a case IN-sensitive map, regardless of the host OS
  PathMap<bool> map(CaseSensitivity::Insensitive);

  map.insert(std::make_pair(PathComponent("foo"), true));
  EXPECT_TRUE(map.at("foo"_pc));
  EXPECT_TRUE(map.at("Foo"_pc));

  EXPECT_FALSE(map.insert(std::make_pair(PathComponent("FOO"), false)).second);
  EXPECT_FALSE(map.emplace(PathComponent("FOO"), false).second);
  EXPECT_EQ(map.size(), 1);
  EXPECT_TRUE(map.at("foo"_pc));
  EXPECT_TRUE(map.at("FOO"_pc));

  EXPECT_EQ(map.erase("FOO"_pc), 1);
  EXPECT_EQ(map.size(), 0);

  // Case insensitive referencing
  map["FOO"_pc] = true;
  map["Foo"_pc] = false;
  // Only one FOO entry
  EXPECT_EQ(map.size(), 1);
  // It shows as false
  EXPECT_EQ(map["FOO"_pc], false);
  // The assignment above didn't change the case of the key!
  EXPECT_EQ(map.begin()->first, "FOO"_pc);
}

TEST(PathMap, caseInSensitiveOrdering) {
  PathMap<bool> map1(CaseSensitivity::Insensitive);
  map1.insert(std::make_pair(PathComponent("e"), true));
  map1.insert(std::make_pair(PathComponent("g"), true));
  map1.insert(std::make_pair(PathComponent("f"), true));

  PathMap<bool> map2(CaseSensitivity::Insensitive);
  map2.insert(std::make_pair(PathComponent("e"), true));
  map2.insert(std::make_pair(PathComponent("g"), true));
  map2.insert(std::make_pair(PathComponent("F"), true));

  EXPECT_EQ(map1.size(), map2.size());

  for (auto it1 = map1.cbegin(), it2 = map2.cbegin(); it1 != map1.cend();
       ++it1, ++it2) {
    EXPECT_STRCASEEQ(
        it1->first.asString().c_str(), it2->first.asString().c_str());
  }
}

TEST(PathMap, caseInSensitiveCopyMove) {
  PathMap<bool> map(CaseSensitivity::Insensitive);
  map.insert(std::make_pair(PathComponent("foo"), true));

  PathMap<bool> copied(map);
  EXPECT_TRUE(copied.at("foo"_pc));
  EXPECT_TRUE(copied.at("Foo"_pc));

  PathMap<bool> copy_assign(CaseSensitivity::Sensitive);
  copy_assign = map;
  EXPECT_TRUE(copy_assign.at("foo"_pc));
  EXPECT_TRUE(copy_assign.at("Foo"_pc));

  PathMap<bool> moved(std::move(map));
  EXPECT_TRUE(moved.at("foo"_pc));
  EXPECT_TRUE(moved.at("Foo"_pc));

  PathMap<bool> move_assign(CaseSensitivity::Sensitive);
  move_assign = std::move(moved);
  EXPECT_TRUE(move_assign.at("foo"_pc));
  EXPECT_TRUE(move_assign.at("Foo"_pc));
}

TEST(PathMap, insert) {
  PathMap<bool> map(kPathMapDefaultCaseSensitive);

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
  PathMap<int> map(
      {
          std::make_pair(PathComponent("foo"), 1),
          std::make_pair(PathComponent("bar"), 2),
          std::make_pair(PathComponent("baz"), 3),
      },
      kPathMapDefaultCaseSensitive);

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
  PathMap<int> map(
      {
          std::make_pair(PathComponent("foo"), 1),
          std::make_pair(PathComponent("bar"), 2),
          std::make_pair(PathComponent("baz"), 3),
      },
      kPathMapDefaultCaseSensitive);
  PathMap<int> other = map;
  EXPECT_EQ(3, other.size());
  EXPECT_EQ(map, other);
}

TEST(PathMap, move) {
  PathMap<int> map(
      {
          std::make_pair(PathComponent("foo"), 1),
          std::make_pair(PathComponent("bar"), 2),
          std::make_pair(PathComponent("baz"), 3),
      },
      kPathMapDefaultCaseSensitive);
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
  PathMap<EmplaceTest> map(kPathMapDefaultCaseSensitive);

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
  PathMap<std::string> b(kPathMapDefaultCaseSensitive),
      a({std::make_pair(PathComponent("foo"), "foo")},
        kPathMapDefaultCaseSensitive);

  b.swap(a);
  EXPECT_EQ(0, a.size()) << "a now has 0 elements";
  EXPECT_EQ(1, b.size()) << "b now has 1 element";
  EXPECT_EQ("foo", b.at("foo"_pc));

  a = std::move(b);
  EXPECT_EQ(1, a.size()) << "a now has 1 element";
  EXPECT_EQ(0, b.size()) << "b now has 0 elements";
  EXPECT_EQ("foo", a.at("foo"_pc));
}
