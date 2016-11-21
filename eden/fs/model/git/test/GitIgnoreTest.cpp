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

#include "eden/fs/model/git/GitIgnore.h"

using namespace facebook::eden;

/*
 * Helper macro to simplify ignore tests.
 *
 * We use a macro here so that failure messages will report the correct
 * line number for the original test statement.  This also makes it simpler to
 * pass in match enums without having to explicitly qualify them with
 * "GitIgnore::" everywhere in the test code.
 */
#define EXPECT_IGNORE(ignore, expected, path)                               \
  do {                                                                      \
    auto expectedResult = GitIgnore::expected;                              \
    auto matchResult = ignore.match(facebook::eden::RelativePath(path));    \
    if (expectedResult != matchResult) {                                    \
      ADD_FAILURE() << "found <" << GitIgnore::matchString(matchResult)     \
                    << "> instead of <"                                     \
                    << GitIgnore::matchString(expectedResult) << "> for \"" \
                    << path << "\"";                                        \
    }                                                                       \
  } while (0)

TEST(GitIgnore, testEmpty) {
  GitIgnore ignore;

  EXPECT_IGNORE(ignore, NO_MATCH, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "bar");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/bar");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/bar/abc");
  EXPECT_IGNORE(ignore, NO_MATCH, "");
}

TEST(GitIgnore, testPrecedence) {
  GitIgnore ignore;
  ignore.loadFile(
      "a*\n"
      "!ab*\n"
      "abc.txt\n"
      "\\!ab*\n");

  EXPECT_IGNORE(ignore, EXCLUDE, "abc.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "ab.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "abc");
  EXPECT_IGNORE(ignore, INCLUDE, "abc.txt2");
  EXPECT_IGNORE(ignore, INCLUDE, "ab");
  EXPECT_IGNORE(ignore, EXCLUDE, "a_xyz");
  EXPECT_IGNORE(ignore, EXCLUDE, "a");
  EXPECT_IGNORE(ignore, EXCLUDE, "!abc");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar");
  EXPECT_IGNORE(ignore, NO_MATCH, "!a");
}

TEST(GitIgnore, testComments) {
  GitIgnore ignore;

  // # is only a comment at the start of a line.
  // Anywhere else it should be treated as a literal '#' character
  ignore.loadFile(
      "#\n"
      "\n"
      "#hello\n"
      "# testing\n"
      "\\#test\n"
      "abc#def\n"
      " #foo\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "hello");
  EXPECT_IGNORE(ignore, NO_MATCH, "#hello");
  EXPECT_IGNORE(ignore, NO_MATCH, "testing");
  EXPECT_IGNORE(ignore, NO_MATCH, "#testing");
  EXPECT_IGNORE(ignore, NO_MATCH, "# testing");
  EXPECT_IGNORE(ignore, NO_MATCH, "test");
  EXPECT_IGNORE(ignore, EXCLUDE, "#test");
  EXPECT_IGNORE(ignore, NO_MATCH, "#test2");
  EXPECT_IGNORE(ignore, EXCLUDE, "abc#def");
  EXPECT_IGNORE(ignore, EXCLUDE, " #foo");
}

TEST(GitIgnore, testNoTerminatingNewline) {
  GitIgnore ignore;
  ignore.loadFile(
      "foobar\n"
      "test.txt");

  EXPECT_IGNORE(ignore, EXCLUDE, "foobar");
  EXPECT_IGNORE(ignore, EXCLUDE, "test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test");
  EXPECT_IGNORE(ignore, NO_MATCH, "example.txt");

  ignore.loadFile("!test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar");
  EXPECT_IGNORE(ignore, INCLUDE, "test.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "some/deep/directory/test.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "x/test.txt");
}

TEST(GitIgnore, testTrailingSpaces) {
  // Unescaped trailing spaces should be ignored
  GitIgnore ignore;
  ignore.loadFile(
      "foobar   \n"
      "withspace\\ \n"
      "3space\\  \\  \n"
      "example   \n");

  EXPECT_IGNORE(ignore, EXCLUDE, "foobar");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar ");
  EXPECT_IGNORE(ignore, EXCLUDE, "withspace ");
  EXPECT_IGNORE(ignore, NO_MATCH, "withspace");
  EXPECT_IGNORE(ignore, EXCLUDE, "3space   ");
  EXPECT_IGNORE(ignore, NO_MATCH, "3space  ");
  EXPECT_IGNORE(ignore, NO_MATCH, "3space    ");
  EXPECT_IGNORE(ignore, NO_MATCH, "3space ");
  EXPECT_IGNORE(ignore, NO_MATCH, "3space");
  EXPECT_IGNORE(ignore, EXCLUDE, "example");
  EXPECT_IGNORE(ignore, NO_MATCH, "example   ");
}

TEST(GitIgnore, testCRLF) {
  // Both CR and CRLF should be handled as line endings.
  // A plain LF is not treated as a line ending, and is considered
  // part of the pattern.
  GitIgnore ignore;
  ignore.loadFile(
      "foobar\r\n"
      "!abc.txt\n"
      "xyz\r"
      "def\n"
      "/example  \r\n"
      "prefix*\r\n");

  EXPECT_IGNORE(ignore, EXCLUDE, "foobar");
  EXPECT_IGNORE(ignore, INCLUDE, "abc.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "xyz");
  EXPECT_IGNORE(ignore, NO_MATCH, "def");
  EXPECT_IGNORE(ignore, EXCLUDE, "xyz\rdef");
  EXPECT_IGNORE(ignore, EXCLUDE, "example");
  EXPECT_IGNORE(ignore, EXCLUDE, "prefix");
  EXPECT_IGNORE(ignore, EXCLUDE, "prefixfoo");
  EXPECT_IGNORE(ignore, EXCLUDE, "prefix.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "x");
}

TEST(GitIgnore, testUTF8BOM) {
  // A leading utf-8 BOM should be ignored
  GitIgnore ignore;
  ignore.loadFile(
      "\xef\xbb\xbf"
      "xyz\n"
      "/test.txt\n");

  EXPECT_IGNORE(ignore, EXCLUDE, "xyz");
  EXPECT_IGNORE(ignore, EXCLUDE, "test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "xyz.txt");

  // Other binary data that isn't a BOM should be included in the pattern
  ignore.loadFile(
      "\xef\xbb\xff"
      "xyz\n"
      "/test.txt\n");

  EXPECT_IGNORE(ignore, NO_MATCH, "xyz");
  EXPECT_IGNORE(ignore, EXCLUDE, "\xef\xbb\xffxyz");
  EXPECT_IGNORE(ignore, EXCLUDE, "test.txt");
}

TEST(GitIgnore, testBasenameMatch) {
  GitIgnore ignore;
  ignore.loadFile(
      "foobar\n"
      "/test.txt\n"
      "abc/def\n"
      "*/file\n"
      "ignoreddir/*\n");

  EXPECT_IGNORE(ignore, EXCLUDE, "foobar");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobarz");
  EXPECT_IGNORE(ignore, NO_MATCH, "zfoobar");
  EXPECT_IGNORE(ignore, EXCLUDE, "a/foobar");
  EXPECT_IGNORE(ignore, EXCLUDE, "a/b/c/foobar");
  // Note: "foobar" in the middle of the path won't match.
  // This will need to be handled by the ignore code by performing ignore
  // processing on each directory as we traverse down into it.
  // (We will probably have to be slightly careful about directories that are
  // ignored but which contain files that have been explicitly requested to be
  // tracked.)
  EXPECT_IGNORE(ignore, NO_MATCH, "a/b/c/foobar/def");

  EXPECT_IGNORE(ignore, EXCLUDE, "test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test.txtz");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/b/c/test.txt");

  EXPECT_IGNORE(ignore, EXCLUDE, "abc/def");
  EXPECT_IGNORE(ignore, NO_MATCH, "x/abc/def");

  EXPECT_IGNORE(ignore, NO_MATCH, "file");
  EXPECT_IGNORE(ignore, EXCLUDE, "a/file");
  EXPECT_IGNORE(ignore, EXCLUDE, "testdir/file");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/b/c/file");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/bfile");

  EXPECT_IGNORE(ignore, NO_MATCH, "ignoreddir");
  EXPECT_IGNORE(ignore, EXCLUDE, "ignoreddir/foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "x/ignoreddir/foo");
}

// TODO: test trailing backslash to ensure it only matches directories
// - test a backslash at the end of a line
// - test a backslash at the end of the file
// - test the pattern "/"

// TODO: test with wildcards: *, ?, [] character classes
// TODO: test various combinations with "**"
// TODO: test special cases that should have optimized code paths:
// - no wildcards
// - endswith patterns like "*.c", "*.txt"

TEST(GitIgnore, testCornerCases) {
  GitIgnore ignore;

  // ! by itself on a line should be ignored
  ignore.loadFile(
      "#\n"
      "!\n"
      "!#\n"
      "!foo\n"
      "\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "");
  EXPECT_IGNORE(ignore, INCLUDE, "#");
  EXPECT_IGNORE(ignore, INCLUDE, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar");

  // Test with just a "/"
  ignore.loadFile(
      "/\n"
      "/#\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "bar");
  EXPECT_IGNORE(ignore, EXCLUDE, "#");

  // Patterns ending in a trailing backslash are invalid and
  // are completely ignored
  ignore.loadFile(
      "test\n"
      "abc\\\n"
      "foo\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc\\");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "test");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");

  // Make sure the file is processed correctly if it ends in a backslash
  // are completely ignored
  ignore.loadFile(
      "foo\n"
      "\\");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo\\");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo\n\\");

  // Multiple leading slashes or trailing slashes can't ever match
  // any real paths, since the paths passed in should never start or end with
  // slashes.
  ignore.loadFile(
      "foo\n"
      "//abc\n"
      "xyz//\n"
      "////\n"
      "//testpath//\n"
      "bar\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, NO_MATCH, "xyz");
  EXPECT_IGNORE(ignore, NO_MATCH, "testpath");
  EXPECT_IGNORE(ignore, NO_MATCH, "test/path");
  EXPECT_IGNORE(ignore, EXCLUDE, "bar");
}
