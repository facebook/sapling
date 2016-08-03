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
#include <boost/functional/hash.hpp>
#include <sstream>
#include "PathFuncs.h"

using facebook::eden::dirname;
using facebook::eden::basename;
using folly::StringPiece;
using std::string;
using std::vector;
using namespace facebook::eden;

TEST(PathFuncs, StringCompare) {
  PathComponentPiece piece("foo");

  EXPECT_EQ("foo", piece);
  EXPECT_EQ(piece, "foo");
}

TEST(PathFuncs, Iterate) {
  RelativePath rel("foo/bar/baz");

  std::vector<RelativePathPiece> parents(
      rel.paths().begin(), rel.paths().end());
  EXPECT_EQ(3, parents.size());
  EXPECT_EQ(RelativePathPiece("foo"), parents.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), parents.at(1));
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), parents.at(2));

  std::vector<RelativePathPiece> allPaths(
      rel.allPaths().begin(), rel.allPaths().end());
  EXPECT_EQ(4, allPaths.size());
  EXPECT_EQ(RelativePathPiece(""), allPaths.at(0));
  EXPECT_EQ(RelativePathPiece("foo"), allPaths.at(1));
  EXPECT_EQ(RelativePathPiece("foo/bar"), allPaths.at(2));
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), allPaths.at(3));

  // And in reverse.
  std::vector<RelativePathPiece> rparents(
      rel.rpaths().begin(), rel.rpaths().end());
  EXPECT_EQ(3, rparents.size());
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), rparents.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), rparents.at(1));
  EXPECT_EQ(RelativePathPiece("foo"), rparents.at(2));

  std::vector<RelativePathPiece> rallPaths(
      rel.rallPaths().begin(), rel.rallPaths().end());
  EXPECT_EQ(4, rallPaths.size());
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), rallPaths.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), rallPaths.at(1));
  EXPECT_EQ(RelativePathPiece("foo"), rallPaths.at(2));
  EXPECT_EQ(RelativePathPiece(""), rallPaths.at(3));

  // An empty relative path yields no elements.
  RelativePath emptyRel;
  std::vector<RelativePathPiece> emptyPaths(
      emptyRel.paths().begin(), emptyRel.paths().end());
  EXPECT_EQ(0, emptyPaths.size());

  std::vector<RelativePathPiece> allEmptyPaths(
      emptyRel.allPaths().begin(), emptyRel.allPaths().end());
  EXPECT_EQ(1, allEmptyPaths.size());
  EXPECT_EQ(RelativePathPiece(""), allEmptyPaths.at(0));

  // An empty relative path yields no elements in reverse either.
  std::vector<RelativePathPiece> remptyPaths(
      emptyRel.rpaths().begin(), emptyRel.rpaths().end());
  EXPECT_EQ(0, remptyPaths.size());
  std::vector<RelativePathPiece> rallEmptyPaths(
      emptyRel.rallPaths().begin(), emptyRel.rallPaths().end());
  EXPECT_EQ(1, rallEmptyPaths.size());
  EXPECT_EQ(RelativePathPiece(""), rallEmptyPaths.at(0));

  AbsolutePath absPath("/foo/bar/baz");
  std::vector<AbsolutePathPiece> acomps(
      absPath.paths().begin(), absPath.paths().end());
  EXPECT_EQ(4, acomps.size());
  EXPECT_EQ(AbsolutePathPiece("/"), acomps.at(0));
  EXPECT_EQ(AbsolutePathPiece("/foo"), acomps.at(1));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar"), acomps.at(2));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar/baz"), acomps.at(3));

  std::vector<AbsolutePathPiece> racomps(
      absPath.rpaths().begin(), absPath.rpaths().end());
  EXPECT_EQ(4, racomps.size());
  EXPECT_EQ(AbsolutePathPiece("/foo/bar/baz"), racomps.at(0));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar"), racomps.at(1));
  EXPECT_EQ(AbsolutePathPiece("/foo"), racomps.at(2));
  EXPECT_EQ(AbsolutePathPiece("/"), racomps.at(3));

  AbsolutePath slashAbs("/");
  std::vector<AbsolutePathPiece> slashPieces(
      slashAbs.paths().begin(), slashAbs.paths().end());
  EXPECT_EQ(1, slashPieces.size());
  EXPECT_EQ(AbsolutePathPiece("/"), slashPieces.at(0));

  std::vector<AbsolutePathPiece> rslashPieces(
      slashAbs.rpaths().begin(), slashAbs.rpaths().end());
  EXPECT_EQ(1, rslashPieces.size());
  EXPECT_EQ(AbsolutePathPiece("/"), rslashPieces.at(0));
}

TEST(PathFuncs, IteratorDecrement) {
  auto checkDecrement = [](
      const auto& path,
      StringPiece function,
      const auto& range,
      const vector<string>& expected) {
    SCOPED_TRACE(folly::to<string>(path, ".", function, "()"));
    auto iter = range.end();
    for (const auto& expectedPath : expected) {
      ASSERT_FALSE(iter == range.begin());
      --iter;
      EXPECT_EQ(expectedPath, (*iter).stringPiece());
    }
    EXPECT_TRUE(iter == range.begin());
  };

  RelativePath rel("foo/bar/baz");
  vector<string> expected = {"foo/bar/baz", "foo/bar", "foo"};
  checkDecrement(rel, "paths", rel.paths(), expected);

  expected = vector<string>{"foo/bar/baz", "foo/bar", "foo", ""};
  checkDecrement(rel, "allPaths", rel.allPaths(), expected);

  expected = vector<string>{"foo", "foo/bar", "foo/bar/baz"};
  checkDecrement(rel, "rpaths", rel.rpaths(), expected);

  expected = vector<string>{"", "foo", "foo/bar", "foo/bar/baz"};
  checkDecrement(rel, "rallPaths", rel.rallPaths(), expected);

  AbsolutePath abs("/foo/bar/baz");
  expected = vector<string>{"/foo/bar/baz", "/foo/bar", "/foo", "/"};
  checkDecrement(abs, "paths", abs.paths(), expected);

  expected = vector<string>{"/", "/foo", "/foo/bar", "/foo/bar/baz"};
  checkDecrement(abs, "rpaths", abs.rpaths(), expected);
}

TEST(PathFuncs, InitializeFromIter) {
  // Assert that we can build a vector of path components and convert
  // it to a RelativePath
  std::vector<PathComponent> components = {
      PathComponent("a"), PathComponent("b"), PathComponent("c")};

  // This form uses iterators explicitly
  RelativePath rel(components.begin(), components.end());
  EXPECT_EQ("a/b/c", rel.stringPiece());

  // This form constructs from the container directly (which uses the
  // iterator form under the covers)
  RelativePath rel2(components);
  EXPECT_EQ(rel, rel2);

  // And this form uses an initializer_list (which also uses the iterator
  // form under the covers).
  // Note that we're mixing both the Stored and Piece flavors of the
  // PathComponent in the initializer.
  RelativePath rel3{PathComponent("stored"), PathComponentPiece("notstored")};
  EXPECT_EQ("stored/notstored", rel3.stringPiece());
}

TEST(PathFuncs, Hash) {
  // Assert that we can find the hash_value function in the correct
  // namespace for boost::hash.
  boost::hash<PathComponentPiece> hasher;
  EXPECT_EQ(9188533406165618471, hasher(PathComponentPiece("foo")));

  // Similarly for std::hash
  std::set<PathComponent> pset;
  std::set<RelativePath> rset;
  std::set<AbsolutePath> aset;

  std::unordered_set<PathComponent> upset;
  std::unordered_set<RelativePath> urset;
  std::unordered_set<AbsolutePath> uaset;
}

TEST(PathFuncs, Stream) {
  // Assert that our stream operator functions.
  std::stringstream str;
  str << PathComponent("file");
  EXPECT_EQ("file", str.str());
}

TEST(PathFuncs, ImplicitPiece) {
  // Assert that we can implicitly convert from Stored -> Piece,
  // which is a pattern we desire for passing either Stored or Piece
  // to a method that accepts a Piece.
  PathComponent comp("stored");
  [](PathComponentPiece piece) {
    EXPECT_EQ("stored", piece.stringPiece());
  }(comp);
}

TEST(PathFuncs, PathComponent) {
  PathComponent comp("hello");
  EXPECT_EQ("hello", comp.stringPiece());

  PathComponentPiece compPiece("helloPiece");
  EXPECT_EQ("helloPiece", compPiece.stringPiece());

  PathComponent storedFromStored(comp);
  EXPECT_EQ(comp, storedFromStored);

  PathComponent storedFromPiece(compPiece);
  EXPECT_EQ(compPiece, storedFromPiece);

  PathComponentPiece pieceFromStored(comp);
  EXPECT_EQ(comp, pieceFromStored);

  PathComponentPiece pieceFromPiece(compPiece);
  EXPECT_EQ(compPiece, pieceFromPiece);

  EXPECT_NE(comp, compPiece);

  EXPECT_THROW(PathComponent("foo/bar"), std::domain_error);
  EXPECT_THROW(PathComponent(""), std::domain_error);
  EXPECT_THROW(PathComponent("."), std::domain_error);
  EXPECT_THROW(PathComponent(".."), std::domain_error);
}

TEST(PathFuncs, RelativePath) {
  RelativePath emptyRel;
  EXPECT_EQ("", emptyRel.stringPiece());
  EXPECT_EQ("", (emptyRel + RelativePath()).value());

  EXPECT_THROW(RelativePath("/foo/bar"), std::domain_error);
  EXPECT_THROW(RelativePath("foo/"), std::domain_error);

  RelativePathPiece relPiece("foo/bar");
  EXPECT_EQ("foo/bar", relPiece.stringPiece());
  EXPECT_NE(emptyRel, relPiece);

  EXPECT_EQ("a", (emptyRel + RelativePathPiece("a")).value());
  EXPECT_EQ("a", (RelativePathPiece("a") + emptyRel).value());

  auto comp = PathComponentPiece("top") + PathComponentPiece("sub");
  EXPECT_EQ("top/sub", comp.stringPiece());

  auto comp2 = comp + PathComponentPiece("third");
  EXPECT_EQ("top/sub/third", comp2.stringPiece());

  auto comp3 = comp + emptyRel;
  EXPECT_EQ("top/sub", comp3.stringPiece());

  auto comp4 = emptyRel + comp;
  EXPECT_EQ("top/sub", comp4.stringPiece());

  EXPECT_EQ("third", comp2.basename().stringPiece());
  EXPECT_EQ("top/sub", comp2.dirname().stringPiece());
  EXPECT_EQ("top", comp2.dirname().dirname().stringPiece());
  EXPECT_EQ("", comp2.dirname().dirname().dirname().stringPiece());
  EXPECT_EQ("", comp2.dirname().dirname().dirname().dirname().stringPiece());
}

TEST(PathFuncs, AbsolutePath) {
  EXPECT_THROW(AbsolutePath("invalid"), std::domain_error);
  EXPECT_THROW(AbsolutePath(""), std::domain_error);
  EXPECT_THROW(AbsolutePath("/trailing/slash/"), std::domain_error);

  AbsolutePath abs("/some/dir");
  EXPECT_EQ("dir", abs.basename().stringPiece());
  EXPECT_EQ("/some", abs.dirname().stringPiece());

  EXPECT_EQ("/some/dir", (abs + RelativePathPiece("")).value());

  auto rel = PathComponentPiece("one") + PathComponentPiece("two");
  auto comp = abs + rel;
  EXPECT_EQ("/some/dir/one/two", comp.stringPiece());

  auto comp2 = abs + RelativePathPiece();
  EXPECT_EQ("/some/dir", comp2.stringPiece());

  auto comp3 = abs + PathComponent("comp");
  EXPECT_EQ("/some/dir/comp", comp3.stringPiece());

  EXPECT_EQ("/", AbsolutePathPiece().stringPiece());
  EXPECT_EQ("/", AbsolutePathPiece("/").stringPiece());
  auto comp4 = AbsolutePathPiece() + RelativePathPiece("foo");
  EXPECT_EQ("/foo", comp4.stringPiece());

  // auto bad = rel + abs; doesn't compile; invalid for ABS to be on RHS
}

TEST(PathFuncs, dirname) {
  EXPECT_EQ("foo/bar", dirname("foo/bar/baz"));
  EXPECT_EQ("foo", dirname("foo/bar"));
  EXPECT_EQ("", dirname("foo"));
}

TEST(PathFuncs, basename) {
  // Note: need explicit StringPiece type here to avoid the compiler picking
  // basename(3) based on type deduction from the const char * literal.
  // Should resolve our our idea of a Path type, but don't want to bikeshed
  // that right here
  EXPECT_EQ("baz", basename(StringPiece("foo/bar/baz")));
  EXPECT_EQ("bar", basename(StringPiece("foo/bar")));
  EXPECT_EQ("foo", basename(StringPiece("foo")));
}

TEST(PathFuncs, isSubDir) {
  // Helper functions that convert string arguments to RelativePathPiece
  auto isSubdir = [](StringPiece a, StringPiece b) {
    return RelativePathPiece(a).isSubDirOf(RelativePathPiece(b));
  };
  auto isParent = [](StringPiece a, StringPiece b) {
    return RelativePathPiece(a).isParentDirOf(RelativePathPiece(b));
  };

  EXPECT_TRUE(isSubdir("foo/bar/baz", ""));
  EXPECT_TRUE(isSubdir("foo/bar/baz", "foo"));
  EXPECT_TRUE(isSubdir("foo/bar/baz", "foo/bar"));
  EXPECT_FALSE(isSubdir("foo/bar/baz", "foo/bar/baz"));
  EXPECT_FALSE(isSubdir("foo/bar", "foo/bar/baz"));
  EXPECT_FALSE(isSubdir("foo/barbaz", "foo/bar"));

  EXPECT_TRUE(isParent("", "foo/bar/baz"));
  EXPECT_TRUE(isParent("foo", "foo/bar/baz"));
  EXPECT_TRUE(isParent("foo/bar", "foo/bar/baz"));
  EXPECT_FALSE(isParent("foo/bar/baz", "foo/bar/baz"));
  EXPECT_FALSE(isParent("foo/bar", "foo/barbaz"));
  EXPECT_FALSE(isParent("foo/bar/baz", "foo/bar"));
}

TEST(PathFuncs, findParent) {
  RelativePath path("foo/bar/baz");

  auto it = path.findParent(RelativePathPiece("foo"));
  vector<RelativePathPiece> parents(it, path.allPaths().end());
  EXPECT_EQ(3, parents.size());
  EXPECT_EQ(RelativePathPiece("foo"), parents.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), parents.at(1));
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), parents.at(2));

  it = path.findParent(RelativePathPiece(""));
  parents = vector<RelativePathPiece>(it, path.allPaths().end());
  EXPECT_EQ(4, parents.size());
  EXPECT_EQ(RelativePathPiece(""), parents.at(0));
  EXPECT_EQ(RelativePathPiece("foo"), parents.at(1));
  EXPECT_EQ(RelativePathPiece("foo/bar"), parents.at(2));
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), parents.at(3));

  it = path.findParent(RelativePathPiece("foo/bar/baz"));
  EXPECT_TRUE(it == path.allPaths().end());
}
