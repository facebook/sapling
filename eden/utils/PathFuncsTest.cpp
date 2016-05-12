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
using namespace facebook::eden;

TEST(PathFuncs, StringCompare) {
  PathComponentPiece piece("foo");

  EXPECT_EQ("foo", piece);
  EXPECT_EQ(piece, "foo");
}

TEST(PathFuncs, Iterate) {
  RelativePath rel("foo/bar/baz");

  std::vector<RelativePathPiece> components(rel.begin(), rel.end());
  EXPECT_EQ(3, components.size());
  EXPECT_EQ(RelativePathPiece("foo"), components.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), components.at(1));
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), components.at(2));

  // And in reverse.
  std::vector<RelativePathPiece> rcomponents(rel.rbegin(), rel.rend());
  EXPECT_EQ(3, rcomponents.size());
  EXPECT_EQ(RelativePathPiece("foo/bar/baz"), rcomponents.at(0));
  EXPECT_EQ(RelativePathPiece("foo/bar"), rcomponents.at(1));
  EXPECT_EQ(RelativePathPiece("foo"), rcomponents.at(2));

  // An empty relative path yields no elements.
  RelativePath emptyRel;
  std::vector<RelativePathPiece> emptyPieces(emptyRel.begin(), emptyRel.end());
  EXPECT_EQ(0, emptyPieces.size());

  // An empty relative path yields no elements in reverse either.
  std::vector<RelativePathPiece> remptyPieces(
      emptyRel.rbegin(), emptyRel.rend());
  EXPECT_EQ(0, remptyPieces.size());

  AbsolutePath absPath("/foo/bar/baz");
  std::vector<AbsolutePathPiece> acomps(absPath.begin(), absPath.end());
  EXPECT_EQ(4, acomps.size());
  EXPECT_EQ(AbsolutePathPiece("/"), acomps.at(0));
  EXPECT_EQ(AbsolutePathPiece("/foo"), acomps.at(1));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar"), acomps.at(2));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar/baz"), acomps.at(3));

  std::vector<AbsolutePathPiece> racomps(absPath.rbegin(), absPath.rend());
  EXPECT_EQ(4, racomps.size());
  EXPECT_EQ(AbsolutePathPiece("/foo/bar/baz"), racomps.at(0));
  EXPECT_EQ(AbsolutePathPiece("/foo/bar"), racomps.at(1));
  EXPECT_EQ(AbsolutePathPiece("/foo"), racomps.at(2));
  EXPECT_EQ(AbsolutePathPiece("/"), racomps.at(3));

  AbsolutePath slashAbs("/");
  std::vector<AbsolutePathPiece> slashPieces(slashAbs.begin(), slashAbs.end());
  EXPECT_EQ(1, slashPieces.size());
  EXPECT_EQ(AbsolutePathPiece("/"), slashPieces.at(0));

  std::vector<AbsolutePathPiece> rslashPieces(slashAbs.begin(), slashAbs.end());
  EXPECT_EQ(1, rslashPieces.size());
  EXPECT_EQ(AbsolutePathPiece("/"), rslashPieces.at(0));
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
