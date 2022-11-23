/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/PathFuncs.h"

#include <boost/functional/hash.hpp>
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/portability/Fcntl.h>
#include <folly/portability/GMock.h>
#include <folly/portability/GTest.h>
#include <folly/portability/SysStat.h>
#include <folly/portability/Unistd.h>
#include <folly/test/TestUtils.h>
#include <sstream>

#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/FileUtils.h"

namespace facebook::eden {

using folly::checkUnixError;
using folly::StringPiece;
using std::string;
using std::vector;
using testing::ElementsAre;

static_assert(std::is_nothrow_default_constructible_v<AbsolutePathPiece>);
static_assert(std::is_nothrow_default_constructible_v<RelativePathPiece>);
// PathComponents are not default-constructible because they may not be empty.
// static_assert(std::is_nothrow_default_constructible_v<PathComponentPiece>);

static_assert(std::is_nothrow_move_constructible_v<AbsolutePathPiece>);
static_assert(std::is_nothrow_move_constructible_v<RelativePathPiece>);
static_assert(std::is_nothrow_move_constructible_v<PathComponentPiece>);

static_assert(std::is_nothrow_move_assignable_v<AbsolutePathPiece>);
static_assert(std::is_nothrow_move_assignable_v<RelativePathPiece>);
static_assert(std::is_nothrow_move_assignable_v<PathComponentPiece>);

static_assert(std::is_nothrow_move_constructible_v<AbsolutePath>);
static_assert(std::is_nothrow_move_constructible_v<RelativePath>);
static_assert(std::is_nothrow_move_constructible_v<PathComponent>);

static_assert(std::is_nothrow_move_assignable_v<AbsolutePath>);
static_assert(std::is_nothrow_move_assignable_v<RelativePath>);
static_assert(std::is_nothrow_move_assignable_v<PathComponent>);

TEST(PathFuncs, Sanity) {
  EXPECT_THROW(PathComponentPiece{"."}, std::domain_error);
  EXPECT_THROW(PathComponentPiece{".."}, std::domain_error);

  EXPECT_THROW(RelativePathPiece{"foo/./bar"}, std::domain_error);
  EXPECT_THROW(RelativePathPiece{"../foo/bar"}, std::domain_error);

  EXPECT_THROW(
      detail::AbsolutePathSanityCheck{}("/foo/../bar"), std::domain_error);
  EXPECT_THROW(
      detail::AbsolutePathSanityCheck{}("/foo/./bar"), std::domain_error);

  if (folly::kIsWindows) {
    EXPECT_THROW(
        detail::AbsolutePathSanityCheck{}("/foo\\../bar"), std::domain_error);
  }
}

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
  EXPECT_EQ("foo"_relpath, parents.at(0));
  EXPECT_EQ("foo/bar"_relpath, parents.at(1));
  EXPECT_EQ("foo/bar/baz"_relpath, parents.at(2));

  std::vector<RelativePathPiece> allPaths(
      rel.allPaths().begin(), rel.allPaths().end());
  EXPECT_EQ(4, allPaths.size());
  EXPECT_EQ(""_relpath, allPaths.at(0));
  EXPECT_EQ("foo"_relpath, allPaths.at(1));
  EXPECT_EQ("foo/bar"_relpath, allPaths.at(2));
  EXPECT_EQ("foo/bar/baz"_relpath, allPaths.at(3));

  // And in reverse.
  std::vector<RelativePathPiece> rparents(
      rel.rpaths().begin(), rel.rpaths().end());
  EXPECT_EQ(3, rparents.size());
  EXPECT_EQ("foo/bar/baz"_relpath, rparents.at(0));
  EXPECT_EQ("foo/bar"_relpath, rparents.at(1));
  EXPECT_EQ("foo"_relpath, rparents.at(2));

  std::vector<RelativePathPiece> rallPaths(
      rel.rallPaths().begin(), rel.rallPaths().end());
  EXPECT_EQ(4, rallPaths.size());
  EXPECT_EQ("foo/bar/baz"_relpath, rallPaths.at(0));
  EXPECT_EQ("foo/bar"_relpath, rallPaths.at(1));
  EXPECT_EQ("foo"_relpath, rallPaths.at(2));
  EXPECT_EQ(""_relpath, rallPaths.at(3));

  if (folly::kIsWindows) {
    RelativePath winRel("foo\\bar/baz");

    std::vector<RelativePathPiece> winParents(
        rel.paths().begin(), rel.paths().end());
    EXPECT_EQ(3, winParents.size());
    EXPECT_EQ("foo"_relpath, winParents.at(0));
    EXPECT_EQ("foo\\bar"_relpath, winParents.at(1));
    EXPECT_EQ("foo\\bar/baz"_relpath, winParents.at(2));

    std::vector<RelativePathPiece> winAllPaths(
        winRel.allPaths().begin(), winRel.allPaths().end());
    EXPECT_EQ(4, winAllPaths.size());
    EXPECT_EQ(""_relpath, winAllPaths.at(0));
    EXPECT_EQ("foo"_relpath, winAllPaths.at(1));
    EXPECT_EQ("foo\\bar"_relpath, winAllPaths.at(2));
    EXPECT_EQ("foo\\bar/baz"_relpath, winAllPaths.at(3));

    // And in reverse.
    std::vector<RelativePathPiece> winRparents(
        winRel.rpaths().begin(), winRel.rpaths().end());
    EXPECT_EQ(3, winRparents.size());
    EXPECT_EQ("foo\\bar/baz"_relpath, winRparents.at(0));
    EXPECT_EQ("foo\\bar"_relpath, winRparents.at(1));
    EXPECT_EQ("foo"_relpath, winRparents.at(2));

    std::vector<RelativePathPiece> winRallPaths(
        winRel.rallPaths().begin(), winRel.rallPaths().end());
    EXPECT_EQ(4, winRallPaths.size());
    EXPECT_EQ("foo\\bar/baz"_relpath, winRallPaths.at(0));
    EXPECT_EQ("foo\\bar"_relpath, winRallPaths.at(1));
    EXPECT_EQ("foo"_relpath, winRallPaths.at(2));
    EXPECT_EQ(""_relpath, winRallPaths.at(3));
  }

  // An empty relative path yields no elements.
  RelativePath emptyRel;
  std::vector<RelativePathPiece> emptyPaths(
      emptyRel.paths().begin(), emptyRel.paths().end());
  EXPECT_EQ(0, emptyPaths.size());

  std::vector<RelativePathPiece> allEmptyPaths(
      emptyRel.allPaths().begin(), emptyRel.allPaths().end());
  EXPECT_EQ(1, allEmptyPaths.size());
  EXPECT_EQ(""_relpath, allEmptyPaths.at(0));

  // An empty relative path yields no elements in reverse either.
  std::vector<RelativePathPiece> remptyPaths(
      emptyRel.rpaths().begin(), emptyRel.rpaths().end());
  EXPECT_EQ(0, remptyPaths.size());
  std::vector<RelativePathPiece> rallEmptyPaths(
      emptyRel.rallPaths().begin(), emptyRel.rallPaths().end());
  EXPECT_EQ(1, rallEmptyPaths.size());
  EXPECT_EQ(""_relpath, rallEmptyPaths.at(0));

  if (folly::kIsWindows) {
    AbsolutePath absPath("\\\\?\\foo\\bar\\baz", detail::SkipPathSanityCheck{});
    std::vector<AbsolutePathPiece> acomps(
        absPath.paths().begin(), absPath.paths().end());
    EXPECT_EQ(4, acomps.size());
    EXPECT_EQ("\\\\?\\", acomps.at(0).view());
    EXPECT_EQ("\\\\?\\foo", acomps.at(1).view());
    EXPECT_EQ("\\\\?\\foo\\bar", acomps.at(2).view());
    EXPECT_EQ("\\\\?\\foo\\bar\\baz", acomps.at(3).view());

    std::vector<AbsolutePathPiece> racomps(
        absPath.rpaths().begin(), absPath.rpaths().end());
    EXPECT_EQ(4, racomps.size());
    EXPECT_EQ("\\\\?\\foo\\bar\\baz", racomps.at(0).view());
    EXPECT_EQ("\\\\?\\foo\\bar", racomps.at(1).view());
    EXPECT_EQ("\\\\?\\foo", racomps.at(2).view());
    EXPECT_EQ("\\\\?\\", racomps.at(3).view());

    AbsolutePath slashAbs("\\\\?\\", detail::SkipPathSanityCheck{});
    std::vector<AbsolutePathPiece> slashPieces(
        slashAbs.paths().begin(), slashAbs.paths().end());
    EXPECT_EQ(1, slashPieces.size());
    EXPECT_EQ("\\\\?\\", slashPieces.at(0).view());

    std::vector<AbsolutePathPiece> rslashPieces(
        slashAbs.rpaths().begin(), slashAbs.rpaths().end());
    EXPECT_EQ(1, rslashPieces.size());
    EXPECT_EQ("\\\\?\\", rslashPieces.at(0).view());
  } else {
    AbsolutePath absPath("/foo/bar/baz", detail::SkipPathSanityCheck{});
    std::vector<AbsolutePathPiece> acomps(
        absPath.paths().begin(), absPath.paths().end());
    EXPECT_EQ(4, acomps.size());
    EXPECT_EQ("/", acomps.at(0).view());
    EXPECT_EQ("/foo", acomps.at(1).view());
    EXPECT_EQ("/foo/bar", acomps.at(2).view());
    EXPECT_EQ("/foo/bar/baz", acomps.at(3).view());

    std::vector<AbsolutePathPiece> racomps(
        absPath.rpaths().begin(), absPath.rpaths().end());
    EXPECT_EQ(4, racomps.size());
    EXPECT_EQ("/foo/bar/baz", racomps.at(0).view());
    EXPECT_EQ("/foo/bar", racomps.at(1).view());
    EXPECT_EQ("/foo", racomps.at(2).view());
    EXPECT_EQ("/", racomps.at(3).view());

    AbsolutePath slashAbs("/", detail::SkipPathSanityCheck{});
    std::vector<AbsolutePathPiece> slashPieces(
        slashAbs.paths().begin(), slashAbs.paths().end());
    EXPECT_EQ(1, slashPieces.size());
    EXPECT_EQ("/", slashPieces.at(0).view());

    std::vector<AbsolutePathPiece> rslashPieces(
        slashAbs.rpaths().begin(), slashAbs.rpaths().end());
    EXPECT_EQ(1, rslashPieces.size());
    EXPECT_EQ("/", rslashPieces.at(0).view());
  }
}

TEST(PathFuncs, IteratorDecrement) {
  auto checkDecrement = [](const auto& path,
                           StringPiece function,
                           const auto& range,
                           const vector<string>& expectedList) {
    SCOPED_TRACE(fmt::format("{}.{}()", path, function));
    auto iter = range.end();
    for (const auto& expectedPath : expectedList) {
      ASSERT_FALSE(iter == range.begin());
      --iter;
      EXPECT_EQ(expectedPath, (*iter).view());
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

  if (folly::kIsWindows) {
    RelativePath winRel("foo\\bar/baz");
    expected = {"foo\\bar/baz", "foo\\bar", "foo"};
    checkDecrement(winRel, "paths", winRel.paths(), expected);

    expected = vector<string>{"foo\\bar/baz", "foo\\bar", "foo", ""};
    checkDecrement(winRel, "allPaths", winRel.allPaths(), expected);

    expected = vector<string>{"foo", "foo\\bar", "foo\\bar/baz"};
    checkDecrement(winRel, "rpaths", winRel.rpaths(), expected);

    expected = vector<string>{"", "foo", "foo\\bar", "foo\\bar/baz"};
    checkDecrement(winRel, "rallPaths", winRel.rallPaths(), expected);
  }

  if (folly::kIsWindows) {
    AbsolutePath abs("\\\\?\\foo\\bar\\baz", detail::SkipPathSanityCheck{});
    expected = vector<string>{
        "\\\\?\\foo\\bar\\baz", "\\\\?\\foo\\bar", "\\\\?\\foo", "\\\\?\\"};
    checkDecrement(abs, "paths", abs.paths(), expected);

    expected = vector<string>{
        "\\\\?\\", "\\\\?\\foo", "\\\\?\\foo\\bar", "\\\\?\\foo\\bar\\baz"};
    checkDecrement(abs, "rpaths", abs.rpaths(), expected);
  } else {
    AbsolutePath abs("/foo/bar/baz", detail::SkipPathSanityCheck{});
    expected = vector<string>{"/foo/bar/baz", "/foo/bar", "/foo", "/"};
    checkDecrement(abs, "paths", abs.paths(), expected);

    expected = vector<string>{"/", "/foo", "/foo/bar", "/foo/bar/baz"};
    checkDecrement(abs, "rpaths", abs.rpaths(), expected);
  }
}

TEST(PathFuncs, IterateComponents) {
  RelativePath rel("foo/bar/baz");
  std::vector<PathComponentPiece> relParts(
      rel.components().begin(), rel.components().end());
  EXPECT_THAT(relParts, ElementsAre("foo"_pc, "bar"_pc, "baz"_pc));

  std::vector<PathComponentPiece> relRParts(
      rel.rcomponents().begin(), rel.rcomponents().end());
  EXPECT_THAT(relRParts, ElementsAre("baz"_pc, "bar"_pc, "foo"_pc));

  if (folly::kIsWindows) {
    RelativePath winRel("foo\\bar/baz");
    std::vector<PathComponentPiece> winRelParts(
        winRel.components().begin(), winRel.components().end());
    EXPECT_THAT(winRelParts, ElementsAre("foo"_pc, "bar"_pc, "baz"_pc));

    std::vector<PathComponentPiece> winRelRParts(
        winRel.rcomponents().begin(), winRel.rcomponents().end());
    EXPECT_THAT(winRelRParts, ElementsAre("baz"_pc, "bar"_pc, "foo"_pc));
  }

  AbsolutePath abs{
      folly::kIsWindows ? "\\\\?\\foo\\bar\\baz" : "/foo/bar/baz",
      detail::SkipPathSanityCheck{}};
  std::vector<PathComponentPiece> absParts(
      abs.components().begin(), abs.components().end());
  EXPECT_THAT(absParts, ElementsAre("foo"_pc, "bar"_pc, "baz"_pc));

  std::vector<PathComponentPiece> absRParts(
      abs.rcomponents().begin(), abs.rcomponents().end());
  EXPECT_THAT(absRParts, ElementsAre("baz"_pc, "bar"_pc, "foo"_pc));

  RelativePath rel2("r/s/t/u");
  std::vector<PathComponentPiece> rel2Parts(
      rel2.components().begin(), rel2.components().end());
  EXPECT_THAT(rel2Parts, ElementsAre("r"_pc, "s"_pc, "t"_pc, "u"_pc));

  std::vector<PathComponentPiece> rel2RParts(
      rel2.rcomponents().begin(), rel2.rcomponents().end());
  EXPECT_THAT(rel2RParts, ElementsAre("u"_pc, "t"_pc, "s"_pc, "r"_pc));

  if (folly::kIsWindows) {
    RelativePath winRel2("r\\s/t\\u");
    std::vector<PathComponentPiece> winRel2Parts(
        winRel2.components().begin(), winRel2.components().end());
    EXPECT_THAT(winRel2Parts, ElementsAre("r"_pc, "s"_pc, "t"_pc, "u"_pc));

    std::vector<PathComponentPiece> winRel2RParts(
        winRel2.rcomponents().begin(), winRel2.rcomponents().end());
    EXPECT_THAT(winRel2RParts, ElementsAre("u"_pc, "t"_pc, "s"_pc, "r"_pc));
  }

  AbsolutePath abs2{
      folly::kIsWindows ? "\\\\?\\a\\b\\c\\d" : "/a/b/c/d",
      detail::SkipPathSanityCheck{}};
  std::vector<PathComponentPiece> abs2Parts(
      abs2.components().begin(), abs2.components().end());
  EXPECT_THAT(abs2Parts, ElementsAre("a"_pc, "b"_pc, "c"_pc, "d"_pc));

  std::vector<PathComponentPiece> abs2RParts(
      abs2.rcomponents().begin(), abs2.rcomponents().end());
  EXPECT_THAT(abs2RParts, ElementsAre("d"_pc, "c"_pc, "b"_pc, "a"_pc));

  RelativePath empty;
  std::vector<PathComponentPiece> emptyParts(
      empty.components().begin(), empty.components().end());
  EXPECT_THAT(emptyParts, ElementsAre());
  std::vector<PathComponentPiece> emptyRParts(
      empty.rcomponents().begin(), empty.rcomponents().end());
  EXPECT_THAT(emptyRParts, ElementsAre());
}

TEST(PathFuncs, IterateSuffixes) {
  RelativePath rel("foo/bar/baz");
  std::vector<RelativePathPiece> relParts(
      rel.suffixes().begin(), rel.suffixes().end());
  EXPECT_THAT(
      relParts,
      ElementsAre("foo/bar/baz"_relpath, "bar/baz"_relpath, "baz"_relpath));

  std::vector<RelativePathPiece> relRParts(
      rel.rsuffixes().begin(), rel.rsuffixes().end());
  EXPECT_THAT(
      relRParts,
      ElementsAre("baz"_relpath, "bar/baz"_relpath, "foo/bar/baz"_relpath));

  if (folly::kIsWindows) {
    RelativePath winRel("foo\\bar/baz");
    std::vector<RelativePathPiece> winRelParts(
        winRel.suffixes().begin(), winRel.suffixes().end());
    EXPECT_THAT(
        winRelParts,
        ElementsAre("foo\\bar/baz"_relpath, "bar/baz"_relpath, "baz"_relpath));

    std::vector<RelativePathPiece> winRelRParts(
        winRel.rsuffixes().begin(), winRel.rsuffixes().end());
    EXPECT_THAT(
        winRelRParts,
        ElementsAre("baz"_relpath, "bar/baz"_relpath, "foo\\bar/baz"_relpath));
  }

  if (folly::kIsWindows) {
    AbsolutePath abs{"\\\\?\\foo\\bar\\baz", detail::SkipPathSanityCheck{}};
    std::vector<RelativePathPiece> absParts(
        abs.suffixes().begin(), abs.suffixes().end());
    EXPECT_THAT(
        absParts,
        ElementsAre(
            "foo\\bar\\baz"_relpath, "bar\\baz"_relpath, "baz"_relpath));

    std::vector<RelativePathPiece> absRParts(
        abs.rsuffixes().begin(), abs.rsuffixes().end());
    EXPECT_THAT(
        absRParts,
        ElementsAre(
            "baz"_relpath, "bar\\baz"_relpath, "foo\\bar\\baz"_relpath));
  } else {
    AbsolutePath abs{"/foo/bar/baz", detail::SkipPathSanityCheck{}};
    std::vector<RelativePathPiece> absParts(
        abs.suffixes().begin(), abs.suffixes().end());
    EXPECT_THAT(
        absParts,
        ElementsAre("foo/bar/baz"_relpath, "bar/baz"_relpath, "baz"_relpath));

    std::vector<RelativePathPiece> absRParts(
        abs.rsuffixes().begin(), abs.rsuffixes().end());
    EXPECT_THAT(
        absRParts,
        ElementsAre("baz"_relpath, "bar/baz"_relpath, "foo/bar/baz"_relpath));
  }

  RelativePath rel2("r/s/t/u");
  std::vector<RelativePathPiece> rel2Parts(
      rel2.suffixes().begin(), rel2.suffixes().end());
  EXPECT_THAT(
      rel2Parts,
      ElementsAre(
          "r/s/t/u"_relpath, "s/t/u"_relpath, "t/u"_relpath, "u"_relpath));

  std::vector<RelativePathPiece> rel2RParts(
      rel2.rsuffixes().begin(), rel2.rsuffixes().end());
  EXPECT_THAT(
      rel2RParts,
      ElementsAre(
          "u"_relpath, "t/u"_relpath, "s/t/u"_relpath, "r/s/t/u"_relpath));

  if (folly::kIsWindows) {
    RelativePath winRel2("r\\s/t\\u");
    std::vector<RelativePathPiece> winRel2Parts(
        winRel2.suffixes().begin(), winRel2.suffixes().end());
    EXPECT_THAT(
        winRel2Parts,
        ElementsAre(
            "r\\s/t\\u"_relpath,
            "s/t\\u"_relpath,
            "t\\u"_relpath,
            "u"_relpath));

    std::vector<RelativePathPiece> winRel2RParts(
        winRel2.rsuffixes().begin(), winRel2.rsuffixes().end());
    EXPECT_THAT(
        winRel2RParts,
        ElementsAre(
            "u"_relpath,
            "t\\u"_relpath,
            "s/t\\u"_relpath,
            "r\\s/t\\u"_relpath));
  }

  if (folly::kIsWindows) {
    AbsolutePath abs2("\\\\?\\a\\b\\c\\d", detail::SkipPathSanityCheck{});
    std::vector<RelativePathPiece> abs2Parts(
        abs2.suffixes().begin(), abs2.suffixes().end());
    EXPECT_THAT(
        abs2Parts,
        ElementsAre(
            "a\\b\\c\\d"_relpath,
            "b\\c\\d"_relpath,
            "c\\d"_relpath,
            "d"_relpath));

    std::vector<RelativePathPiece> abs2RParts(
        abs2.rsuffixes().begin(), abs2.rsuffixes().end());
    EXPECT_THAT(
        abs2RParts,
        ElementsAre(
            "d"_relpath,
            "c\\d"_relpath,
            "b\\c\\d"_relpath,
            "a\\b\\c\\d"_relpath));
  } else {
    AbsolutePath abs2("/a/b/c/d", detail::SkipPathSanityCheck{});
    std::vector<RelativePathPiece> abs2Parts(
        abs2.suffixes().begin(), abs2.suffixes().end());
    EXPECT_THAT(
        abs2Parts,
        ElementsAre(
            "a/b/c/d"_relpath, "b/c/d"_relpath, "c/d"_relpath, "d"_relpath));

    std::vector<RelativePathPiece> abs2RParts(
        abs2.rsuffixes().begin(), abs2.rsuffixes().end());
    EXPECT_THAT(
        abs2RParts,
        ElementsAre(
            "d"_relpath, "c/d"_relpath, "b/c/d"_relpath, "a/b/c/d"_relpath));
  }

  RelativePath empty;
  std::vector<RelativePathPiece> emptyParts(
      empty.suffixes().begin(), empty.suffixes().end());
  EXPECT_THAT(emptyParts, ElementsAre());
  std::vector<RelativePathPiece> emptyRParts(
      empty.rsuffixes().begin(), empty.rsuffixes().end());
  EXPECT_THAT(emptyRParts, ElementsAre());
}

TEST(PathFuncs, InitializeFromIter) {
  // Assert that we can build a vector of path components and convert
  // it to a RelativePath
  std::vector<PathComponent> components = {
      PathComponent("a"), PathComponent("b"), PathComponent("c")};

  // This form uses iterators explicitly
  RelativePath rel(components.begin(), components.end());
  EXPECT_EQ("a/b/c", rel.view());

  // This form constructs from the container directly (which uses the
  // iterator form under the covers)
  RelativePath rel2(components);
  EXPECT_EQ(rel, rel2);

  // And this form uses an initializer_list (which also uses the iterator
  // form under the covers).
  // Note that we're mixing both the Stored and Piece flavors of the
  // PathComponent in the initializer.
  RelativePath rel3{PathComponent("stored"), "notstored"_pc};
  EXPECT_EQ("stored/notstored", rel3.view());
}

TEST(PathFuncs, Hash20) {
  // Assert that we can find the hash_value function in the correct
  // namespace for boost::hash.
  boost::hash<PathComponentPiece> hasher;
  EXPECT_EQ(9188533406165618471, hasher("foo"_pc));

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
  [](PathComponentPiece piece) { EXPECT_EQ("stored", piece.view()); }(comp);
}

TEST(PathFuncs, PathComponent) {
  PathComponent comp("hello");
  EXPECT_EQ("hello", comp.view());

  PathComponentPiece compPiece("helloPiece");
  EXPECT_EQ("helloPiece", compPiece.view());

  PathComponent storedFromStored(comp);
  EXPECT_EQ(comp, storedFromStored);

  PathComponent storedFromPiece(compPiece);
  EXPECT_EQ(compPiece, storedFromPiece);

  PathComponentPiece pieceFromStored(comp);
  EXPECT_EQ(comp, pieceFromStored);

  PathComponentPiece pieceFromPiece(compPiece);
  EXPECT_EQ(compPiece, pieceFromPiece);

  EXPECT_NE(comp, compPiece);

  EXPECT_THROW_RE(
      PathComponent("foo/bar"),
      std::domain_error,
      "containing a directory separator");
  EXPECT_THROW_RE(
      PathComponent(""),
      std::domain_error,
      "cannot have an empty PathComponent");
  EXPECT_THROW_RE(PathComponent("."), std::domain_error, "must not be \\.");
  EXPECT_THROW_RE(PathComponent(".."), std::domain_error, "must not be \\.\\.");
}

TEST(PathFuncs, RelativePath) {
  RelativePath emptyRel;
  EXPECT_EQ("", emptyRel.view());
  EXPECT_EQ("", (emptyRel + RelativePath()).value());

  EXPECT_THROW_RE(RelativePath("/foo/bar"), std::domain_error, "absolute path");
  EXPECT_THROW_RE(
      RelativePath("foo/"), std::domain_error, "must not end with a slash");

  RelativePathPiece relPiece("foo/bar");
  EXPECT_EQ("foo/bar", relPiece.view());
  EXPECT_NE(emptyRel, relPiece);

  EXPECT_EQ("a", (emptyRel + "a"_relpath).value());
  EXPECT_EQ("a", ("a"_relpath + emptyRel).value());

  auto comp = "top"_pc + "sub"_pc;
  EXPECT_EQ("top/sub", comp.view());

  auto comp2 = comp + "third"_pc;
  EXPECT_EQ("top/sub/third", comp2.view());

  auto comp3 = comp + emptyRel;
  EXPECT_EQ("top/sub", comp3.view());

  auto comp4 = emptyRel + comp;
  EXPECT_EQ("top/sub", comp4.view());

  EXPECT_EQ("third", comp2.basename().view());
  EXPECT_EQ("top/sub", comp2.dirname().view());
  EXPECT_EQ("top", comp2.dirname().dirname().view());
  EXPECT_EQ("", comp2.dirname().dirname().dirname().view());
  EXPECT_EQ("", comp2.dirname().dirname().dirname().dirname().view());
}

TEST(PathFuncs, AbsolutePath) {
  EXPECT_THROW_RE(
      detail::AbsolutePathSanityCheck{}("invalid"),
      std::domain_error,
      "non-absolute string");
  EXPECT_THROW_RE(
      detail::AbsolutePathSanityCheck{}(""),
      std::domain_error,
      "non-absolute string");
  if (folly::kIsWindows) {
    EXPECT_THROW_RE(
        detail::AbsolutePathSanityCheck{}("\\\\?\\trailing\\slash/"),
        std::domain_error,
        "must not end with a slash");
  } else {
    EXPECT_THROW_RE(
        detail::AbsolutePathSanityCheck{}("/trailing/slash/"),
        std::domain_error,
        "must not end with a slash");
  }

  if (folly::kIsWindows) {
    AbsolutePath abs("\\\\?\\some\\dir", detail::SkipPathSanityCheck{});
    EXPECT_EQ("dir", abs.basename().view());
    EXPECT_EQ("\\\\?\\some", abs.dirname().view());

    EXPECT_EQ("\\\\?\\some\\dir", (abs + ""_relpath).value());

    auto rel = "one"_pc + "two"_pc;
    auto comp = abs + rel;
    EXPECT_EQ("\\\\?\\some\\dir\\one\\two", comp.view());

    auto comp2 = abs + RelativePathPiece();
    EXPECT_EQ("\\\\?\\some\\dir", comp2.view());

    auto comp3 = abs + PathComponent("comp");
    EXPECT_EQ("\\\\?\\some\\dir\\comp", comp3.view());

    EXPECT_EQ("\\\\?\\", AbsolutePathPiece().view());
    EXPECT_EQ(
        "\\\\?\\",
        AbsolutePathPiece("\\\\?\\", detail::SkipPathSanityCheck{}).view());
    auto comp4 = AbsolutePathPiece() + "foo"_relpath;
    EXPECT_EQ("\\\\?\\foo", comp4.view());

    AbsolutePath root{};
    EXPECT_EQ(RelativePathPiece(), root.relativize(root));
    EXPECT_EQ(RelativePathPiece(), abs.relativize(abs));

    EXPECT_EQ("foo"_relpath, abs.relativize(abs + "foo"_relpath));
    EXPECT_EQ("foo\\bar"_relpath, abs.relativize(abs + "foo/bar"_relpath));
  } else {
    AbsolutePath abs("/some/dir", detail::SkipPathSanityCheck{});
    EXPECT_EQ("dir", abs.basename().view());
    EXPECT_EQ("/some", abs.dirname().view());

    EXPECT_EQ("/some/dir", (abs + ""_relpath).value());

    auto rel = "one"_pc + "two"_pc;
    auto comp = abs + rel;
    EXPECT_EQ("/some/dir/one/two", comp.view());

    auto comp2 = abs + RelativePathPiece();
    EXPECT_EQ("/some/dir", comp2.view());

    auto comp3 = abs + PathComponent("comp");
    EXPECT_EQ("/some/dir/comp", comp3.view());

    EXPECT_EQ("/", AbsolutePathPiece().view());
    EXPECT_EQ(
        "/", AbsolutePathPiece("/", detail::SkipPathSanityCheck{}).view());
    auto comp4 = AbsolutePathPiece() + "foo"_relpath;
    EXPECT_EQ("/foo", comp4.view());

    AbsolutePath root{};
    EXPECT_EQ(RelativePathPiece(), root.relativize(root));
    EXPECT_EQ(RelativePathPiece(), abs.relativize(abs));

    EXPECT_EQ("foo"_relpath, abs.relativize(abs + "foo"_relpath));
    EXPECT_EQ("foo/bar"_relpath, abs.relativize(abs + "foo/bar"_relpath));
  }

  // auto bad = rel + abs; doesn't compile; invalid for ABS to be on RHS
}

TEST(PathFuncs, relativize_memory_safety) {
  if (folly::kIsWindows) {
    AbsolutePath abs{
        "\\\\?\\some\\dir\\this\\has\\to\\be\\long\\enough\\to\\exceed\\sso",
        detail::SkipPathSanityCheck{}};

    // This test validates that the result is accessible as long as the
    // argument's memory is alive.
    const auto& child = abs + "foo"_relpath;
    auto piece = abs.relativize(child);
    EXPECT_EQ("foo"_relpath, piece);
  } else {
    AbsolutePath abs{
        "/some/dir/this/has/to/be/long/enough/to/exceed/sso",
        detail::SkipPathSanityCheck{}};

    // This test validates that the result is accessible as long as the
    // argument's memory is alive.
    const auto& child = abs + "foo"_relpath;
    auto piece = abs.relativize(child);
    EXPECT_EQ("foo"_relpath, piece);
  }
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

  auto it = path.findParent("foo"_relpath);
  vector<RelativePathPiece> parents(it, path.allPaths().end());
  EXPECT_EQ(3, parents.size());
  EXPECT_EQ("foo"_relpath, parents.at(0));
  EXPECT_EQ("foo/bar"_relpath, parents.at(1));
  EXPECT_EQ("foo/bar/baz"_relpath, parents.at(2));

  it = path.findParent(""_relpath);
  parents = vector<RelativePathPiece>(it, path.allPaths().end());
  EXPECT_EQ(4, parents.size());
  EXPECT_EQ(""_relpath, parents.at(0));
  EXPECT_EQ("foo"_relpath, parents.at(1));
  EXPECT_EQ("foo/bar"_relpath, parents.at(2));
  EXPECT_EQ("foo/bar/baz"_relpath, parents.at(3));

  it = path.findParent("foo/bar/baz"_relpath);
  EXPECT_TRUE(it == path.allPaths().end());
}

TEST(PathFuncs, fmt) {
  // Test using fmt::format with all of the various path types
  PathComponentPiece comp("foo");
  EXPECT_EQ("x(foo)", fmt::format("x({})", comp));

  PathComponentPiece compPiece("bar");
  EXPECT_EQ("x(bar)", fmt::format("x({})", compPiece));

  if (folly::kIsWindows) {
    AbsolutePath abs("\\\\?\\home\\johndoe", detail::SkipPathSanityCheck{});
    EXPECT_EQ("x(\\\\?\\home\\johndoe)", fmt::format("x({})", abs));

    AbsolutePathPiece absPiece(
        "\\\\?\\var\\log\\clowntown", detail::SkipPathSanityCheck{});
    EXPECT_EQ("x(\\\\?\\var\\log\\clowntown)", fmt::format("x({})", absPiece));
  } else {
    AbsolutePath abs("/home/johndoe", detail::SkipPathSanityCheck{});
    EXPECT_EQ("x(/home/johndoe)", fmt::format("x({})", abs));

    AbsolutePathPiece absPiece(
        "/var/log/clowntown", detail::SkipPathSanityCheck{});
    EXPECT_EQ("x(/var/log/clowntown)", fmt::format("x({})", absPiece));
  }

  RelativePath rel("src/ping.c");
  EXPECT_EQ("x(src/ping.c)", fmt::format("x({})", rel));

  RelativePathPiece relPiece("src/abc.def");
  EXPECT_EQ("x(src/abc.def)", fmt::format("x({})", relPiece));
}

namespace {
template <typename T>
std::string fmt_to_string(const T& value) {
  return fmt::to_string(value);
}
} // namespace

TEST(PathFuncs, fmt_const_ref) {
  EXPECT_EQ("foo", fmt_to_string(PathComponentPiece{"foo"}));
  EXPECT_EQ("foo", fmt_to_string(RelativePathPiece{"foo"}));
  if (folly::kIsWindows) {
    EXPECT_EQ(
        "\\\\?\\foo",
        fmt_to_string(
            AbsolutePathPiece{"\\\\?\\foo", detail::SkipPathSanityCheck{}}));
  } else {
    EXPECT_EQ(
        "/foo",
        fmt_to_string(
            AbsolutePathPiece{"/foo", detail::SkipPathSanityCheck{}}));
  }
}

TEST(PathFuncs, splitFirst) {
  using SplitResult = decltype(splitFirst(std::declval<RelativePath>()));

  RelativePath rp1{""};
  EXPECT_THROW(splitFirst(rp1), std::domain_error);

  RelativePath rp2{"foo"};
  EXPECT_EQ((SplitResult{"foo", ""}), splitFirst(rp2));

  RelativePath rp3{"foo/bar"};
  EXPECT_EQ((SplitResult{"foo", "bar"}), splitFirst(rp3));

  RelativePath rp4{"foo/bar/baz"};
  EXPECT_EQ((SplitResult{"foo", "bar/baz"}), splitFirst(rp4));
}

namespace {
/*
 * Helper class to create a temporary directory and cd into it while this
 * object exists.
 */
class TmpWorkingDir {
 public:
  TmpWorkingDir() {
    checkUnixError(chdir(pathStr.c_str()), "failed to chdir");
  }
  ~TmpWorkingDir() {
    checkUnixError(chdir(oldDir.value().c_str()), "failed to chdir");
  }

  AbsolutePath oldDir{getcwd()};
  folly::test::TemporaryDirectory dir = makeTempDir();
  std::string pathStr{dir.path().string()};
  AbsolutePathPiece path{canonicalPath(pathStr)};
};
} // namespace

TEST(PathFuncs, canonicalPath) {
  auto fooBarAbcTxt =
      folly::kIsWindows ? "\\\\?\\foo\\bar\\abc.txt" : "/foo/bar/abc.txt";
  EXPECT_EQ(fooBarAbcTxt, canonicalPath("/foo/bar/abc.txt").value());
  EXPECT_EQ(fooBarAbcTxt, canonicalPath("///foo/bar//abc.txt").value());
  EXPECT_EQ(fooBarAbcTxt, canonicalPath("///foo/bar/./abc.txt").value());
  EXPECT_EQ(fooBarAbcTxt, canonicalPath("/..//foo/bar//abc.txt").value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\abc.txt" : "/abc.txt",
      canonicalPath("/..//foo/bar/../../abc.txt").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("/").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("////").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("/../../..").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("/././.").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("/./././").value());
  EXPECT_EQ(detail::kRootStr, canonicalPath("/./.././").value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\abc.foo" : "/abc.foo",
      canonicalPath("/abc.foo/../abc.foo///").value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\foo" : "/foo",
      canonicalPath("//foo").value());

  auto base = AbsolutePath{
      folly::kIsWindows ? "\\\\?\\base\\dir\\path" : "/base/dir/path",
      detail::SkipPathSanityCheck{}};
  auto baseDirPath =
      folly::kIsWindows ? "\\\\?\\base\\dir\\path" : "/base/dir/path";
  EXPECT_EQ(baseDirPath, canonicalPath("", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\path\\abc" : "/base/dir/path/abc",
      canonicalPath("abc", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\path\\abc\\def"
                        : "/base/dir/path/abc/def",
      canonicalPath("abc/def/", base).value());
  EXPECT_EQ(baseDirPath, canonicalPath(".", base).value());
  EXPECT_EQ(baseDirPath, canonicalPath("././/.", base).value());
  auto baseDir = folly::kIsWindows ? "\\\\?\\base\\dir" : "/base/dir";
  EXPECT_EQ(baseDir, canonicalPath("..", base).value());
  EXPECT_EQ(baseDir, canonicalPath("../", base).value());
  EXPECT_EQ(baseDir, canonicalPath("../.", base).value());
  EXPECT_EQ(baseDir, canonicalPath(".././", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\xy\\s.txt" : "/base/dir/xy/s.txt",
      canonicalPath(".././xy//z/../s.txt", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\xy\\s.txt" : "/base/dir/xy/s.txt",
      canonicalPath("z//.././../xy//s.txt", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\path\\ foo bar "
                        : "/base/dir/path/ foo bar ",
      canonicalPath(" foo bar ", base).value());
  EXPECT_EQ(
      folly::kIsWindows ? "\\\\?\\base\\dir\\path\\...\\test"
                        : "/base/dir/path/.../test",
      canonicalPath(".../test", base).value());

  // TODO(T66260288): These tests currently do not pass on Windows, as
  // canonicalPath() incorrectly tries to put a leading slash on the paths.
  // e.g., "C:/foo" ends up being converted to "/C:/foo"
  if (!folly::kIsWindows) {
    TmpWorkingDir tmpDir;
    EXPECT_EQ(tmpDir.pathStr, canonicalPath(".").value());
    EXPECT_EQ(tmpDir.pathStr + "/foo", canonicalPath("foo").value());
    EXPECT_EQ(
        tmpDir.pathStr + "/a/b/c.txt",
        canonicalPath("foo/../a//d/../b/./c.txt").value());
  }
}

TEST(PathFuncs, joinAndNormalize) {
  const auto good = [](const char* base, const char* path) {
    return joinAndNormalize(RelativePath{base}, path).value();
  };
  const auto bad = [](const char* base, const char* path) {
    return joinAndNormalize(RelativePath{base}, path).error();
  };

  EXPECT_EQ(good("a/b/c", "d"), RelativePath{"a/b/c/d"});
  EXPECT_EQ(good("a/b/c/d", "../../e"), RelativePath{"a/b/e"});
  EXPECT_EQ(good("a/b/c", ""), RelativePath{"a/b/c"});
  EXPECT_EQ(good("", ""), RelativePath{""});
  EXPECT_EQ(good("", "a/b"), RelativePath{"a/b"});
  EXPECT_EQ(good("a/b", "../.."), RelativePath{""});
  EXPECT_EQ(good("a/b/c", "../.."), RelativePath{"a"});

  EXPECT_EQ(bad("a", "/b/c"), EPERM);
  EXPECT_EQ(bad("a/b/c", "/"), EPERM);

  EXPECT_EQ(bad("", ".."), EXDEV);
  EXPECT_EQ(bad("a/b", "../../.."), EXDEV);
  EXPECT_EQ(bad("a", "b/../../.."), EXDEV);
}

// Disable the realpath tests on Windows, since we normally don't have
// permissions to create symlinks.
#ifndef _WIN32
TEST(PathFuncs, realpath) {
  TmpWorkingDir tmpDir;

  // Change directories to the tmp dir for the duration of this test
  auto oldDir = getcwd();
  SCOPE_EXIT {
    checkUnixError(chdir(oldDir.value().c_str()), "failed to chdir");
  };
  checkUnixError(chdir(tmpDir.pathStr.c_str()), "failed to chdir");

  // Set up some files to test with
  folly::checkUnixError(
      open("simple.txt", O_WRONLY | O_CREAT, 0644),
      "failed to create simple.txt");
  folly::checkUnixError(mkdir("parent", 0755), "failed to mkdir parent");
  folly::checkUnixError(mkdir("parent/child", 0755), "failed to mkdir child");
  folly::checkUnixError(
      open("parent/child/file.txt", O_WRONLY | O_CREAT, 0644),
      "failed to create file.txt");
  folly::checkUnixError(
      symlink("parent//child/../child/file.txt", "wonky_link"),
      "failed to create wonky_link");
  folly::checkUnixError(
      symlink("child/nowhere", "parent/broken_link"),
      "failed to create broken_link");
  folly::checkUnixError(
      symlink("../loop_b", "parent/loop_a"), "failed to create loop_a");
  folly::checkUnixError(
      symlink("parent/loop_a", "loop_b"), "failed to create loop_b");

  // Now actually test realpath()
  EXPECT_EQ(tmpDir.pathStr + "/simple.txt", realpath("simple.txt").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/simple.txt", realpath("parent/../simple.txt").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/simple.txt",
      realpath("parent/..//parent/.//child/../../simple.txt").value());
  EXPECT_THROW_ERRNO(realpath("nosuchdir/../simple.txt"), ENOENT);
  EXPECT_EQ(
      tmpDir.pathStr + "/simple.txt",
      realpath(tmpDir.pathStr + "//simple.txt").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/simple.txt",
      realpath(tmpDir.pathStr + "//parent/../simple.txt").value());

  EXPECT_EQ(
      tmpDir.pathStr + "/parent/child/file.txt",
      realpath("parent///child//file.txt").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/parent/child/file.txt",
      realpath("wonky_link").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/parent/child/file.txt",
      realpathExpected("wonky_link").value().value());

  EXPECT_EQ(
      tmpDir.pathStr + "/parent/child", realpath("parent///child//").value());
  EXPECT_EQ(tmpDir.pathStr + "/parent", realpath("parent/.").value());
  EXPECT_EQ(tmpDir.pathStr, realpath("parent/..").value());

  EXPECT_THROW_ERRNO(realpath("parent/broken_link"), ENOENT);
  EXPECT_THROW_ERRNO(realpath("parent/loop_a"), ELOOP);
  EXPECT_THROW_ERRNO(realpath("loop_b"), ELOOP);
  EXPECT_THROW_ERRNO(realpath("parent/nosuchfile"), ENOENT);
  EXPECT_EQ(ELOOP, realpathExpected("parent/loop_a").error());
  EXPECT_EQ(ENOENT, realpathExpected("parent/nosuchfile").error());

  // Perform some tests for normalizeBestEffort() as well
  EXPECT_EQ(
      tmpDir.pathStr + "/simple.txt",
      normalizeBestEffort(tmpDir.pathStr + "//simple.txt").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/parent/nosuchfile",
      normalizeBestEffort("parent/nosuchfile"));
  EXPECT_EQ(
      tmpDir.pathStr + "/nosuchfile",
      normalizeBestEffort("parent/..//nosuchfile"));
  EXPECT_EQ(
      tmpDir.pathStr + "/parent/loop_a", normalizeBestEffort("parent/loop_a"));
  EXPECT_EQ(
      "/foo/bar/abc.txt", normalizeBestEffort("/..//foo/bar//abc.txt").value());
  EXPECT_EQ(tmpDir.pathStr, normalizeBestEffort(tmpDir.pathStr));
}
#endif // !_WIN32

TEST(PathFuncs, expandUser) {
  if (folly::kIsWindows) {
    EXPECT_EQ(
        AbsolutePathPiece("\\\\?\\foo\\bar", detail::SkipPathSanityCheck{}),
        expandUser("\\\\?\\foo\\bar"));
  } else {
    EXPECT_EQ(
        AbsolutePathPiece("/foo/bar", detail::SkipPathSanityCheck{}),
        expandUser("/foo/bar"));
  }
  EXPECT_THROW(expandUser("~user/foo/bar"), std::runtime_error);
  EXPECT_THROW(expandUser("~user/foo/bar", ""), std::runtime_error);
  folly::StringPiece homeBob =
      folly::kIsWindows ? "\\\\?\\home\\bob" : "/home/bob";
  EXPECT_EQ(
      AbsolutePathPiece(
          folly::kIsWindows ? "\\\\?\\home\\bob\\foo\\bar"
                            : "/home/bob/foo/bar",
          detail::SkipPathSanityCheck{}),
      expandUser("~/foo/bar", homeBob));
  EXPECT_EQ(
      AbsolutePath(homeBob, detail::SkipPathSanityCheck{}),
      expandUser("~", homeBob));
  if (folly::kIsWindows) {
    EXPECT_EQ(
        AbsolutePathPiece(
            "\\\\?\\home\\bob\\foo", detail::SkipPathSanityCheck{}),
        expandUser("~//foo/./bar/../", "\\\\?\\home/./bob/"));
  } else {
    EXPECT_EQ(
        AbsolutePathPiece("/home/bob/foo", detail::SkipPathSanityCheck{}),
        expandUser("~//foo/./bar/../", "/home/./bob/"));
  }
}

template <typename StoredType, typename PieceType>
void compareHelper(StringPiece str1, StringPiece str2) {
  auto stored1 = StoredType{str1, detail::SkipPathSanityCheck{}};
  auto stored2 = StoredType{str2, detail::SkipPathSanityCheck{}};
  auto piece1 = PieceType{str1, detail::SkipPathSanityCheck{}};
  auto piece2 = PieceType{str2, detail::SkipPathSanityCheck{}};

  EXPECT_TRUE(stored1 < stored2);
  EXPECT_TRUE(piece1 < piece2);
  EXPECT_TRUE(stored1 < piece2);
  EXPECT_TRUE(piece1 < stored2);

  EXPECT_TRUE(stored1 <= stored2);
  EXPECT_TRUE(piece1 <= piece2);
  EXPECT_TRUE(stored1 <= piece2);
  EXPECT_TRUE(piece1 <= stored2);

  EXPECT_FALSE(stored1 > stored2);
  EXPECT_FALSE(piece1 > piece2);
  EXPECT_FALSE(stored1 > piece2);
  EXPECT_FALSE(piece1 > stored2);

  EXPECT_FALSE(stored1 >= stored2);
  EXPECT_FALSE(piece1 >= piece2);
  EXPECT_FALSE(stored1 >= piece2);
  EXPECT_FALSE(piece1 >= stored2);

  EXPECT_FALSE(stored1 == stored2);
  EXPECT_FALSE(piece1 == piece2);
  EXPECT_FALSE(stored1 == piece2);
  EXPECT_FALSE(piece1 == stored2);

  EXPECT_TRUE(stored1 != stored2);
  EXPECT_TRUE(piece1 != piece2);
  EXPECT_TRUE(stored1 != piece2);
  EXPECT_TRUE(piece1 != stored2);
}

TEST(PathFuncs, comparison) {
  // Test various combinations of path comparison operators,
  // mostly to make sure that the template instantiations all compile
  // correctly and unambiguously.
  compareHelper<PathComponent, PathComponentPiece>("abc", "def");
  compareHelper<RelativePath, RelativePathPiece>("abc/def", "abc/xyz");
  compareHelper<AbsolutePath, AbsolutePathPiece>(
      folly::kIsWindows ? "\\\\?\\abc\\def" : "/abc/def",
      folly::kIsWindows ? "\\\\?\\abc\\xyz" : "/abc/xyz");

  if (folly::kIsWindows) {
    // Make sure that path comparing paths doesn't take into account the path
    // separator.
    compareHelper<RelativePath, RelativePathPiece>("abc/def", "abc\\xyz");

    EXPECT_TRUE(RelativePath{"abc/def"} == RelativePath{"abc\\def"});
    EXPECT_TRUE(RelativePath{"abc/def"} == RelativePathPiece{"abc\\def"});
  }

  // We should always perform byte-by-byte comparisons (and ignore locale)
  EXPECT_LT(PathComponent{"ABC"}, PathComponent{"abc"});
  EXPECT_LT(PathComponent{"XYZ"}, PathComponent{"abc"});
}

TEST(PathFuncs, comparisonInsensitive) {
  auto result =
      comparePathPiece("foo"_pc, "FOO"_pc, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::EQUAL);

  result = comparePathPiece("foo"_pc, "bar"_pc, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::AFTER);

  result = comparePathPiece("foo"_pc, "BAR"_pc, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::AFTER);

  result = comparePathPiece(
      "foo/bar"_relpath, "FOO/bar"_relpath, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::EQUAL);

  result = comparePathPiece(
      "foo/foo"_relpath, "foo/bar"_relpath, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::AFTER);

  result = comparePathPiece(
      "foo/foo"_relpath, "foo/BAR"_relpath, CaseSensitivity::Insensitive);
  EXPECT_EQ(result, CompareResult::AFTER);
}

TEST(PathFuncs, localDirCreateRemove) {
  folly::test::TemporaryDirectory dir = makeTempDir();
  string pathStr{dir.path().string()};
  AbsolutePath tmpDirPath = canonicalPath(pathStr);

  // Create a deep directory, and write a file inside it.
  auto testPath = tmpDirPath + "foo/bar/asdf/test.txt"_relpath;
  ensureDirectoryExists(testPath.dirname());
  writeFile(testPath, StringPiece("test\n")).throwUnlessValue();

  // Read it back just as a sanity check
  auto contents = readFile(testPath);
  ASSERT_TRUE(contents.hasValue());
  EXPECT_EQ("test\n", contents.value());

  // Delete the first-level directory and its contents
  auto topDir = tmpDirPath + "foo"_pc;
  struct stat st;
  auto returnCode = lstat(topDir.c_str(), &st);
  EXPECT_EQ(0, returnCode);
  ASSERT_TRUE(removeRecursively(topDir));
  returnCode = lstat(topDir.c_str(), &st);
  EXPECT_NE(0, returnCode);
  EXPECT_EQ(ENOENT, errno);

  // Calling removeRecursively() on a non-existent directory should return false
  ASSERT_FALSE(removeRecursively(topDir));
}

TEST(PathFuncs, noThrow) {
  // if std::string is nothrow move constructible and assignable, the
  // path types should be as well.
  if (std::is_nothrow_move_constructible<std::string>::value) {
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<AbsolutePath>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<AbsolutePathPiece>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<RelativePath>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<RelativePathPiece>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<PathComponent>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_constructible<PathComponentPiece>::value);
  }

  if (std::is_nothrow_move_assignable<std::string>::value) {
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<AbsolutePath>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<AbsolutePathPiece>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<RelativePath>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<RelativePathPiece>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<PathComponent>::value);
    ASSERT_TRUE(
        detail::kPathsAreCopiedOnMove ||
        std::is_nothrow_move_assignable<PathComponentPiece>::value);
  }
}

#ifdef _WIN32
TEST(PathFuncs, PathComponentWide) {
  PathComponent comp(L"hello");
  EXPECT_EQ("hello", comp.view());
  EXPECT_EQ(L"hello", comp.wide());

  EXPECT_THROW_RE(
      PathComponent(L"foo/bar"),
      std::domain_error,
      "containing a directory separator");

  EXPECT_THROW_RE(
      PathComponent(L"foo\\bar"),
      std::domain_error,
      "containing a directory separator");

  EXPECT_THROW_RE(
      PathComponent(L""),
      std::domain_error,
      "cannot have an empty PathComponent");
  EXPECT_THROW_RE(PathComponent(L"."), std::domain_error, "must not be \\.");
  EXPECT_THROW_RE(
      PathComponent(L".."), std::domain_error, "must not be \\.\\.");
}

TEST(PathFuncs, RelativePathWide) {
  RelativePath emptyRel;
  EXPECT_EQ(L"", emptyRel.wide());

  EXPECT_THROW_RE(
      RelativePath(L"/foo/bar"), std::domain_error, "absolute path");
  // TODO(T66260288): re-enable once fixed.
  // EXPECT_THROW_RE(RelativePath(L"T:/foo/bar"), std::domain_error, "absolute
  // path"); EXPECT_THROW_RE(RelativePath(L"T:\\foo\\bar"), std::domain_error,
  // "absolute path");
  EXPECT_THROW_RE(
      RelativePath(L"foo/"), std::domain_error, "must not end with a slash");
  EXPECT_THROW_RE(
      RelativePath(L"foo\\"), std::domain_error, "must not end with a slash");

  RelativePath relPath(L"foo/bar");
  EXPECT_EQ(L"foo\\bar", relPath.wide());
  EXPECT_EQ("foo", relPath.dirname());
  EXPECT_EQ("bar", relPath.basename());

  RelativePath relPathBack(L"foo\\bar");
  EXPECT_EQ(L"foo\\bar", relPathBack.wide());
  EXPECT_EQ("foo", relPathBack.dirname());
  EXPECT_EQ("bar", relPathBack.basename());
}
#endif

TEST(PathFuncs, HashEquality) {
  RelativePathPiece rel1{"foo/bar/baz"};
  RelativePathPiece rel2{"foo/bar/baz"};

  std::hash<RelativePathPiece> relHasher{};
  EXPECT_EQ(relHasher(rel1), relHasher(rel2));

  if (folly::kIsWindows) {
    RelativePathPiece winRel{"foo\\bar/baz"};
    EXPECT_EQ(relHasher(rel1), relHasher(winRel));
  }

  AbsolutePathPiece abs1{
      folly::kIsWindows ? "\\\\?\\foo\\bar\\baz" : "/foo/bar/baz",
      detail::SkipPathSanityCheck{}};
  AbsolutePathPiece abs2{
      folly::kIsWindows ? "\\\\?\\foo\\bar\\baz" : "/foo/bar/baz",
      detail::SkipPathSanityCheck{}};

  std::hash<AbsolutePathPiece> absHasher{};
  EXPECT_EQ(absHasher(abs1), absHasher(abs2));
}

TEST(PathFuncs, move_or_copy) {
  class T {
   public:
    T(uint32_t* copied, uint32_t* moved) : copied{copied}, moved{moved} {}
    T(const T& other) : copied{other.copied}, moved{other.moved} {
      *copied += 1;
    }
    T(T&& other) noexcept : copied{other.copied}, moved{other.moved} {
      *moved += 1;
    }
    T& operator=(const T& other) {
      copied = other.copied;
      moved = other.moved;
      *copied += 1;
      return *this;
    }
    T& operator=(T&& other) {
      copied = other.copied;
      moved = other.moved;
      *moved += 1;
      return *this;
    }

   private:
    uint32_t* copied;
    uint32_t* moved;
  };

  uint32_t copied = 0;
  uint32_t moved = 0;

  T t1{&copied, &moved};
  [[maybe_unused]] auto t2 = detail::move_or_copy(t1);
  if (detail::kPathsAreCopiedOnMove) {
    EXPECT_EQ(copied, 1);
    EXPECT_EQ(moved, 1);
  } else {
    EXPECT_EQ(copied, 0);
    EXPECT_EQ(moved, 1);
  }
}

} // namespace facebook::eden
