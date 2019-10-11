/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/PathFuncs.h"

#include <boost/functional/hash.hpp>
#include <fcntl.h>
#include <folly/Exception.h>
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <folly/folly-config.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <sys/stat.h>
#include <unistd.h>
#include <sstream>

#include "eden/fs/testharness/TempFile.h"

using facebook::eden::basename;
using facebook::eden::dirname;
using folly::checkUnixError;
using folly::StringPiece;
using std::string;
using std::vector;
using namespace facebook::eden;
using testing::ElementsAre;

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

  AbsolutePath absPath("/foo/bar/baz");
  std::vector<AbsolutePathPiece> acomps(
      absPath.paths().begin(), absPath.paths().end());
  EXPECT_EQ(4, acomps.size());
  EXPECT_EQ("/"_abspath, acomps.at(0));
  EXPECT_EQ("/foo"_abspath, acomps.at(1));
  EXPECT_EQ("/foo/bar"_abspath, acomps.at(2));
  EXPECT_EQ("/foo/bar/baz"_abspath, acomps.at(3));

  std::vector<AbsolutePathPiece> racomps(
      absPath.rpaths().begin(), absPath.rpaths().end());
  EXPECT_EQ(4, racomps.size());
  EXPECT_EQ("/foo/bar/baz"_abspath, racomps.at(0));
  EXPECT_EQ("/foo/bar"_abspath, racomps.at(1));
  EXPECT_EQ("/foo"_abspath, racomps.at(2));
  EXPECT_EQ("/"_abspath, racomps.at(3));

  AbsolutePath slashAbs("/");
  std::vector<AbsolutePathPiece> slashPieces(
      slashAbs.paths().begin(), slashAbs.paths().end());
  EXPECT_EQ(1, slashPieces.size());
  EXPECT_EQ("/"_abspath, slashPieces.at(0));

  std::vector<AbsolutePathPiece> rslashPieces(
      slashAbs.rpaths().begin(), slashAbs.rpaths().end());
  EXPECT_EQ(1, rslashPieces.size());
  EXPECT_EQ("/"_abspath, rslashPieces.at(0));
}

TEST(PathFuncs, IteratorDecrement) {
  auto checkDecrement = [](const auto& path,
                           StringPiece function,
                           const auto& range,
                           const vector<string>& expectedList) {
    SCOPED_TRACE(folly::to<string>(path, ".", function, "()"));
    auto iter = range.end();
    for (const auto& expectedPath : expectedList) {
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

TEST(PathFuncs, IterateComponents) {
  RelativePath rel("foo/bar/baz");
  std::vector<PathComponentPiece> relParts(
      rel.components().begin(), rel.components().end());
  EXPECT_THAT(relParts, ElementsAre("foo"_pc, "bar"_pc, "baz"_pc));

  std::vector<PathComponentPiece> relRParts(
      rel.rcomponents().begin(), rel.rcomponents().end());
  EXPECT_THAT(relRParts, ElementsAre("baz"_pc, "bar"_pc, "foo"_pc));

  AbsolutePath abs("/foo/bar/baz");
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

  AbsolutePath abs2("/a/b/c/d");
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

  AbsolutePath abs("/foo/bar/baz");
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

  AbsolutePath abs2("/a/b/c/d");
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
  EXPECT_EQ("a/b/c", rel.stringPiece());

  // This form constructs from the container directly (which uses the
  // iterator form under the covers)
  RelativePath rel2(components);
  EXPECT_EQ(rel, rel2);

  // And this form uses an initializer_list (which also uses the iterator
  // form under the covers).
  // Note that we're mixing both the Stored and Piece flavors of the
  // PathComponent in the initializer.
  RelativePath rel3{PathComponent("stored"), "notstored"_pc};
  EXPECT_EQ("stored/notstored", rel3.stringPiece());
}

TEST(PathFuncs, Hash) {
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

  EXPECT_THROW_RE(
      PathComponent("foo/bar"),
      std::domain_error,
      "containing a directory separator");
  EXPECT_THROW_RE(
      PathComponent(""),
      std::domain_error,
      "cannot have an empty PathComponent");
  EXPECT_THROW_RE(
      PathComponent("."), std::domain_error, "must not be \\. or \\.\\.");
  EXPECT_THROW_RE(
      PathComponent(".."), std::domain_error, "must not be \\. or \\.\\.");
}

TEST(PathFuncs, RelativePath) {
  RelativePath emptyRel;
  EXPECT_EQ("", emptyRel.stringPiece());
  EXPECT_EQ("", (emptyRel + RelativePath()).value());

  EXPECT_THROW_RE(RelativePath("/foo/bar"), std::domain_error, "absolute path");
  EXPECT_THROW_RE(
      RelativePath("foo/"), std::domain_error, "must not end with a slash");

  RelativePathPiece relPiece("foo/bar");
  EXPECT_EQ("foo/bar", relPiece.stringPiece());
  EXPECT_NE(emptyRel, relPiece);

  EXPECT_EQ("a", (emptyRel + "a"_relpath).value());
  EXPECT_EQ("a", ("a"_relpath + emptyRel).value());

  auto comp = "top"_pc + "sub"_pc;
  EXPECT_EQ("top/sub", comp.stringPiece());

  auto comp2 = comp + "third"_pc;
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
  EXPECT_THROW_RE(
      AbsolutePath("invalid"), std::domain_error, "non-absolute string");
  EXPECT_THROW_RE(AbsolutePath(""), std::domain_error, "non-absolute string");
  EXPECT_THROW_RE(
      AbsolutePath("/trailing/slash/"),
      std::domain_error,
      "must not end with a slash");

  AbsolutePath abs("/some/dir");
  EXPECT_EQ("dir", abs.basename().stringPiece());
  EXPECT_EQ("/some", abs.dirname().stringPiece());

  EXPECT_EQ("/some/dir", (abs + ""_relpath).value());

  auto rel = "one"_pc + "two"_pc;
  auto comp = abs + rel;
  EXPECT_EQ("/some/dir/one/two", comp.stringPiece());

  auto comp2 = abs + RelativePathPiece();
  EXPECT_EQ("/some/dir", comp2.stringPiece());

  auto comp3 = abs + PathComponent("comp");
  EXPECT_EQ("/some/dir/comp", comp3.stringPiece());

  EXPECT_EQ("/", AbsolutePathPiece().stringPiece());
  EXPECT_EQ("/", "/"_abspath.stringPiece());
  auto comp4 = AbsolutePathPiece() + "foo"_relpath;
  EXPECT_EQ("/foo", comp4.stringPiece());

  AbsolutePath root("/");
  EXPECT_EQ(RelativePathPiece(), root.relativize(root));
  EXPECT_EQ(RelativePathPiece(), abs.relativize(abs));

  EXPECT_EQ("foo"_relpath, abs.relativize(abs + "foo"_relpath));
  EXPECT_EQ("foo/bar"_relpath, abs.relativize(abs + "foo/bar"_relpath));

  // auto bad = rel + abs; doesn't compile; invalid for ABS to be on RHS
}

TEST(PathFuncs, relativize_memory_safety) {
  AbsolutePath abs{"/some/dir/this/has/to/be/long/enough/to/exceed/sso"};

  // This test validates that the result is accessible as long as the
  // argument's memory is alive.
  const auto& child = abs + "foo"_relpath;
  auto piece = abs.relativize(child);
  EXPECT_EQ("foo"_relpath, piece);
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

TEST(PathFuncs, format) {
  // Test using folly::format with all of the various path types
  PathComponentPiece comp("foo");
  EXPECT_EQ("x(foo)", folly::sformat("x({})", comp));

  PathComponentPiece compPiece("bar");
  EXPECT_EQ("x(bar)", folly::sformat("x({})", compPiece));

  AbsolutePath abs("/home/johndoe");
  EXPECT_EQ("x(/home/johndoe)", folly::sformat("x({})", abs));

  AbsolutePathPiece absPiece("/var/log/clowntown");
  EXPECT_EQ("x(/var/log/clowntown)", folly::sformat("x({})", absPiece));

  RelativePath rel("src/ping.c");
  EXPECT_EQ("x(src/ping.c)", folly::sformat("x({})", rel));

  RelativePathPiece relPiece("src/abc.def");
  EXPECT_EQ("x(src/abc.def)", folly::sformat("x({})", relPiece));
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
  AbsolutePathPiece path{pathStr};
};
} // namespace

TEST(PathFuncs, canonicalPath) {
  EXPECT_EQ("/foo/bar/abc.txt", canonicalPath("/foo/bar/abc.txt").value());
  EXPECT_EQ("/foo/bar/abc.txt", canonicalPath("///foo/bar//abc.txt").value());
  EXPECT_EQ("/foo/bar/abc.txt", canonicalPath("///foo/bar/./abc.txt").value());
  EXPECT_EQ("/foo/bar/abc.txt", canonicalPath("/..//foo/bar//abc.txt").value());
  EXPECT_EQ("/abc.txt", canonicalPath("/..//foo/bar/../../abc.txt").value());
  EXPECT_EQ("/", canonicalPath("/").value());
  EXPECT_EQ("/", canonicalPath("////").value());
  EXPECT_EQ("/", canonicalPath("/../../..").value());
  EXPECT_EQ("/", canonicalPath("/././.").value());
  EXPECT_EQ("/", canonicalPath("/./././").value());
  EXPECT_EQ("/", canonicalPath("/./.././").value());
  EXPECT_EQ("/abc.foo", canonicalPath("/abc.foo/../abc.foo///").value());
  EXPECT_EQ("/foo", canonicalPath("//foo").value());

  auto base = AbsolutePath{"/base/dir/path"};
  EXPECT_EQ("/base/dir/path", canonicalPath("", base).value());
  EXPECT_EQ("/base/dir/path/abc", canonicalPath("abc", base).value());
  EXPECT_EQ("/base/dir/path/abc/def", canonicalPath("abc/def/", base).value());
  EXPECT_EQ("/base/dir/path", canonicalPath(".", base).value());
  EXPECT_EQ("/base/dir/path", canonicalPath("././/.", base).value());
  EXPECT_EQ("/base/dir", canonicalPath("..", base).value());
  EXPECT_EQ("/base/dir", canonicalPath("../", base).value());
  EXPECT_EQ("/base/dir", canonicalPath("../.", base).value());
  EXPECT_EQ("/base/dir", canonicalPath(".././", base).value());
  EXPECT_EQ(
      "/base/dir/xy/s.txt", canonicalPath(".././xy//z/../s.txt", base).value());
  EXPECT_EQ(
      "/base/dir/xy/s.txt",
      canonicalPath("z//.././../xy//s.txt", base).value());
  EXPECT_EQ(
      "/base/dir/path/ foo bar ", canonicalPath(" foo bar ", base).value());
  EXPECT_EQ("/base/dir/path/.../test", canonicalPath(".../test", base).value());

  TmpWorkingDir tmpDir;
  EXPECT_EQ(tmpDir.pathStr, canonicalPath(".").value());
  EXPECT_EQ(tmpDir.pathStr + "/foo", canonicalPath("foo").value());
  EXPECT_EQ(
      tmpDir.pathStr + "/a/b/c.txt",
      canonicalPath("foo/../a//d/../b/./c.txt").value());
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

TEST(PathFuncs, expandUser) {
  EXPECT_EQ("/foo/bar"_abspath, expandUser("/foo/bar"));
  EXPECT_THROW(expandUser("~user/foo/bar"), std::runtime_error);
  EXPECT_THROW(expandUser("~user/foo/bar", ""), std::runtime_error);
  EXPECT_EQ("/home/bob/foo/bar"_abspath, expandUser("~/foo/bar", "/home/bob"));
  EXPECT_EQ("/home/bob"_abspath, expandUser("~", "/home/bob"));
  EXPECT_EQ(
      "/home/bob/foo"_abspath, expandUser("~//foo/./bar/../", "/home/./bob/"));
}

template <typename StoredType, typename PieceType>
void compareHelper(StringPiece str1, StringPiece str2) {
  EXPECT_TRUE(StoredType{str1} < StoredType{str2});
  EXPECT_TRUE(PieceType{str1} < PieceType{str2});
  EXPECT_TRUE(StoredType{str1} < PieceType{str2});
  EXPECT_TRUE(PieceType{str1} < StoredType{str2});

  EXPECT_TRUE(StoredType{str1} <= StoredType{str2});
  EXPECT_TRUE(PieceType{str1} <= PieceType{str2});
  EXPECT_TRUE(StoredType{str1} <= PieceType{str2});
  EXPECT_TRUE(PieceType{str1} <= StoredType{str2});

  EXPECT_FALSE(StoredType{str1} > StoredType{str2});
  EXPECT_FALSE(PieceType{str1} > PieceType{str2});
  EXPECT_FALSE(StoredType{str1} > PieceType{str2});
  EXPECT_FALSE(PieceType{str1} > StoredType{str2});

  EXPECT_FALSE(StoredType{str1} >= StoredType{str2});
  EXPECT_FALSE(PieceType{str1} >= PieceType{str2});
  EXPECT_FALSE(StoredType{str1} >= PieceType{str2});
  EXPECT_FALSE(PieceType{str1} >= StoredType{str2});

  EXPECT_FALSE(StoredType{str1} == StoredType{str2});
  EXPECT_FALSE(PieceType{str1} == PieceType{str2});
  EXPECT_FALSE(StoredType{str1} == PieceType{str2});
  EXPECT_FALSE(PieceType{str1} == StoredType{str2});

  EXPECT_TRUE(StoredType{str1} != StoredType{str2});
  EXPECT_TRUE(PieceType{str1} != PieceType{str2});
  EXPECT_TRUE(StoredType{str1} != PieceType{str2});
  EXPECT_TRUE(PieceType{str1} != StoredType{str2});
}

TEST(PathFuncs, comparison) {
  // Test various combinations of path comparison operators,
  // mostly to make sure that the template instantiations all compile
  // correctly and unambiguously.
  compareHelper<PathComponent, PathComponentPiece>("abc", "def");
  compareHelper<RelativePath, RelativePathPiece>("abc/def", "abc/xyz");
  compareHelper<AbsolutePath, AbsolutePathPiece>("/abc/def", "/abc/xyz");

  // We should always perform byte-by-byte comparisons (and ignore locale)
  EXPECT_LT(PathComponent{"ABC"}, PathComponent{"abc"});
  EXPECT_LT(PathComponent{"XYZ"}, PathComponent{"abc"});
}

TEST(PathFuncs, localDirCreateRemove) {
  folly::test::TemporaryDirectory dir = makeTempDir();
  string pathStr{dir.path().string()};
  AbsolutePathPiece tmpDirPath{pathStr};

  // Create a deep directory, and write a file inside it.
  auto testPath = tmpDirPath + "foo/bar/asdf/test.txt"_relpath;
  ensureDirectoryExists(testPath.dirname());
  folly::writeFile(StringPiece("test\n"), testPath.c_str());

  // Read it back just as a sanity check
  string contents;
  ASSERT_TRUE(folly::readFile(testPath.c_str(), contents));
  EXPECT_EQ("test\n", contents);

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
    ASSERT_TRUE(std::is_nothrow_move_constructible<AbsolutePath>::value);
    ASSERT_TRUE(std::is_nothrow_move_constructible<AbsolutePathPiece>::value);
    ASSERT_TRUE(std::is_nothrow_move_constructible<RelativePath>::value);
    ASSERT_TRUE(std::is_nothrow_move_constructible<RelativePathPiece>::value);
    ASSERT_TRUE(std::is_nothrow_move_constructible<PathComponent>::value);
    ASSERT_TRUE(std::is_nothrow_move_constructible<PathComponentPiece>::value);
  }

  if (std::is_nothrow_move_assignable<std::string>::value) {
    ASSERT_TRUE(std::is_nothrow_move_assignable<AbsolutePath>::value);
    ASSERT_TRUE(std::is_nothrow_move_assignable<AbsolutePathPiece>::value);
    ASSERT_TRUE(std::is_nothrow_move_assignable<RelativePath>::value);
    ASSERT_TRUE(std::is_nothrow_move_assignable<RelativePathPiece>::value);
    ASSERT_TRUE(std::is_nothrow_move_assignable<PathComponent>::value);
    ASSERT_TRUE(std::is_nothrow_move_assignable<PathComponentPiece>::value);
  }
}
