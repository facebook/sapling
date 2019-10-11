/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
#define EXPECT_IGNORE_WITH_TYPE(ignore, expected, path, fileType)           \
  do {                                                                      \
    auto expectedResult = GitIgnore::expected;                              \
    auto matchResult =                                                      \
        (ignore).match(facebook::eden::RelativePath(path), (fileType));     \
    if (expectedResult != matchResult) {                                    \
      ADD_FAILURE() << "found <" << GitIgnore::matchString(matchResult)     \
                    << "> instead of <"                                     \
                    << GitIgnore::matchString(expectedResult) << "> for \"" \
                    << path << "\"";                                        \
    }                                                                       \
  } while (0)

#define EXPECT_IGNORE(ignore, expected, path) \
  EXPECT_IGNORE_WITH_TYPE(ignore, expected, path, GitIgnore::TYPE_FILE)

#define EXPECT_IGNORE_DIR(ignore, expected, path) \
  EXPECT_IGNORE_WITH_TYPE(ignore, expected, path, GitIgnore::TYPE_DIR)

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

TEST(GitIgnore, testStar) {
  GitIgnore ignore;

  // Test some simple "endswith" patterns, plus * in the middle of a path
  ignore.loadFile(
      "*.txt\n"
      "!*.c\n"
      ".*.swp\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "test.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/test.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/bar/abc/test.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "test.c");
  EXPECT_IGNORE(ignore, INCLUDE, "foo/test.c");
  EXPECT_IGNORE(ignore, INCLUDE, "foo/bar/abc/test.c");
  EXPECT_IGNORE(ignore, NO_MATCH, "test.cc");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/test.cc");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/bar/abc/test.cc");
  EXPECT_IGNORE(ignore, EXCLUDE, ".test.txt.swp");
  EXPECT_IGNORE(ignore, EXCLUDE, ".test.swp");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/.test.txt.swp");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/bar/abc/.test.txt.swp");
  EXPECT_IGNORE(ignore, NO_MATCH, ".test.swp.foo");
  EXPECT_IGNORE(ignore, NO_MATCH, ".test.swp.");
  EXPECT_IGNORE(ignore, NO_MATCH, "test.swp");
  EXPECT_IGNORE(ignore, EXCLUDE, ".test.swp.txt");

  ignore.loadFile(
      "*/main.py\n"
      "test/*\n"
      "\n");
  EXPECT_IGNORE(ignore, NO_MATCH, "main.py");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/main.py");
  EXPECT_IGNORE(ignore, EXCLUDE, "main.py/main.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/bar/main.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "test");
  EXPECT_IGNORE(ignore, EXCLUDE, "test/hello.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/test/hello.py");
  // This one won't match by itself, but our implementation should have
  // found that "test/foo" was ignored before trying to check the ignore status
  // of files inside of that directory.
  EXPECT_IGNORE(ignore, NO_MATCH, "test/foo/hello.py");
}

TEST(GitIgnore, testStarStar) {
  GitIgnore ignore;

  // Test leading "**/"
  ignore.loadFile(
      "**/abc/def.txt\n"
      "**/foo.txt\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "abc/def.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/abc/def.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/y/z/abc/def.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/\xff\xff/abc/def.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "def.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/foo.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/y/z/foo.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/\xff\xff/foo.txt");

  // Test trailing "/**"
  ignore.loadFile(
      "abc/**\n"
      "x/y/z/**\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "abc/foo.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "def/abc/foo.txt");
  // We shouldn't match abc itself, only things inside it
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, EXCLUDE, "x/y/z/foo.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "1/2/3/x/y/z/foo.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "x/z/foo.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "y/z/foo.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/y/z/foo.txt");

  // Test both leading "**/" and trailing "/**"
  ignore.loadFile(
      "**/xyz/**\n"
      "!**/readme.txt\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "xyz/foo.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "a/xyz/foo.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "a/b/c/xyz/test/foo.txt");
  EXPECT_IGNORE(ignore, INCLUDE, "a/xyz/readme.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "a/xyz");

  // Test "/**/"
  ignore.loadFile(
      "foo/**/bar.txt\n"
      "**/abc/**/def/*.txt\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/bar.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/1/bar.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo/1/2/3/bar.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/1/2/3/test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test/1/2/3/bar.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "bar.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "1/foo/bar.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/bar.txt/test");
  EXPECT_IGNORE(ignore, EXCLUDE, "abc/def/readme.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc/def/readme.c");
  EXPECT_IGNORE(ignore, EXCLUDE, "abc/foo/def/readme.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc/foo/def/1/readme.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "ab/foo/def/readme.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo/def/1/2/abc/readme.txt");

  // The gitignore(5) man page says that "**" is invalid outside of a leading
  // "**/", a trailing "/**", or "/**/".  Our code (and git's) does process
  // "**" in other locations, but we don't include tests for it here since the
  // behavior is technically undefined.
}

TEST(GitIgnore, testQMark) {
  GitIgnore ignore;

  ignore.loadFile(
      "myfile?txt\n"
      "test??txt\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "myfile.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "myfile_txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "subdir/myfile\x01txt");
  // Filenames are processed as binary.  A question mark should not match a
  // multibyte UTF-8 character.  It should match it byte-by-byte, though.
  EXPECT_IGNORE(ignore, NO_MATCH, "myfile\xc2\xa9txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "test\xc2\xa9txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "test__txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "test??txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "test**txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test?txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "test*txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "testtxt");
  EXPECT_IGNORE(ignore, NO_MATCH, "txt");

  ignore.loadFile(
      "?\n"
      "???\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "t");
  EXPECT_IGNORE(ignore, EXCLUDE, "?");
  EXPECT_IGNORE(ignore, EXCLUDE, "_");
  EXPECT_IGNORE(ignore, EXCLUDE, "\xff");
  EXPECT_IGNORE(ignore, EXCLUDE, "txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "...");
  EXPECT_IGNORE(ignore, NO_MATCH, "tt");
  EXPECT_IGNORE(ignore, EXCLUDE, "example/1");
  EXPECT_IGNORE(ignore, EXCLUDE, "example/txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "example/tt");

  ignore.loadFile(
      "?*?\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "tt");
  EXPECT_IGNORE(ignore, EXCLUDE, "abcdefghi");
  EXPECT_IGNORE(ignore, NO_MATCH, "x");
  EXPECT_IGNORE(ignore, NO_MATCH, "1/23/45/6");

  ignore.loadFile(
      "*abc?\n"
      "foo?bar*\n"
      "123*?456\n"
      "\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "abcd");
  EXPECT_IGNORE(ignore, EXCLUDE, "123abcd");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc");
  EXPECT_IGNORE(ignore, NO_MATCH, "abcde");
  EXPECT_IGNORE(ignore, NO_MATCH, "123abcde");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo_bar");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo_bar123");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo.bar123");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar123");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar");
  EXPECT_IGNORE(ignore, NO_MATCH, "foobar1");
  EXPECT_IGNORE(ignore, EXCLUDE, "123_456");
  EXPECT_IGNORE(ignore, EXCLUDE, "123___456");
  EXPECT_IGNORE(ignore, NO_MATCH, "123456");
  EXPECT_IGNORE(ignore, NO_MATCH, "123_4567");
  EXPECT_IGNORE(ignore, NO_MATCH, "0123_456");
}

TEST(GitIgnore, testCharClass) {
  GitIgnore ignore;
  ignore.loadFile(
      "[abc].txt\n"
      "![!abc].py\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "a.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "b.txt");
  EXPECT_IGNORE(ignore, EXCLUDE, "c.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "d.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "`.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "ab.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc.txt");
  EXPECT_IGNORE(ignore, NO_MATCH, "a.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "b.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "c.py");
  EXPECT_IGNORE(ignore, INCLUDE, "d.py");
  EXPECT_IGNORE(ignore, INCLUDE, "`.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "ab.py");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc.py");

  ignore.loadFile(
      "*.[oa]\n"
      "!*.[ch]\n");
  EXPECT_IGNORE(ignore, INCLUDE, "foo.c");
  EXPECT_IGNORE(ignore, INCLUDE, "foo.h");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo.o");
  EXPECT_IGNORE(ignore, EXCLUDE, "libfoo.a");
  EXPECT_IGNORE(ignore, NO_MATCH, "libfoo.so");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo.ch");
  EXPECT_IGNORE(ignore, INCLUDE, "1/2/3/foo.c");
  EXPECT_IGNORE(ignore, EXCLUDE, "1/2/3/libfoo.a");

  // Test ranges
  ignore.loadFile(
      "foo\n"
      "test[a-m]test\n"
      "abc[x-z]def\n"
      "123[z-a]456\n"
      "789[z-]012\n"
      "x[-y]z\n"
      "hello[!-a]world\n"
      "one[A-Z-9]range\n"
      "bar\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, EXCLUDE, "bar");
  EXPECT_IGNORE(ignore, EXCLUDE, "testatest");
  EXPECT_IGNORE(ignore, EXCLUDE, "testktest");
  EXPECT_IGNORE(ignore, EXCLUDE, "testmtest");
  EXPECT_IGNORE(ignore, NO_MATCH, "testKtest");
  EXPECT_IGNORE(ignore, EXCLUDE, "abcxdef");
  EXPECT_IGNORE(ignore, EXCLUDE, "abcydef");
  EXPECT_IGNORE(ignore, EXCLUDE, "abczdef");
  EXPECT_IGNORE(ignore, NO_MATCH, "abcwdef");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc{def");
  EXPECT_IGNORE(ignore, NO_MATCH, "123z456");
  EXPECT_IGNORE(ignore, EXCLUDE, "789z012");
  EXPECT_IGNORE(ignore, EXCLUDE, "789-012");
  EXPECT_IGNORE(ignore, NO_MATCH, "789x012");
  EXPECT_IGNORE(ignore, EXCLUDE, "x-z");
  EXPECT_IGNORE(ignore, EXCLUDE, "xyz");
  EXPECT_IGNORE(ignore, NO_MATCH, "xYz");
  EXPECT_IGNORE(ignore, EXCLUDE, "hello world");
  EXPECT_IGNORE(ignore, NO_MATCH, "hello-world");
  EXPECT_IGNORE(ignore, NO_MATCH, "helloaworld");
  EXPECT_IGNORE(ignore, EXCLUDE, "oneXrange");
  EXPECT_IGNORE(ignore, EXCLUDE, "one-range");
  EXPECT_IGNORE(ignore, EXCLUDE, "one9range");
  EXPECT_IGNORE(ignore, NO_MATCH, "one8range");

  // Test character class expressions
  ignore.loadFile(
      "foo\n"
      "x[[:alpha:]]\n"
      "y[^x[:upper:]z]\n"
      "z[[:digit:]-z]\n"
      "0[[:alpha]]\n"
      "bar\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, EXCLUDE, "bar");
  EXPECT_IGNORE(ignore, EXCLUDE, "xa");
  EXPECT_IGNORE(ignore, EXCLUDE, "xZ");
  EXPECT_IGNORE(ignore, NO_MATCH, "x1");
  EXPECT_IGNORE(ignore, EXCLUDE, "ya");
  EXPECT_IGNORE(ignore, EXCLUDE, "y.");
  EXPECT_IGNORE(ignore, NO_MATCH, "yA");
  EXPECT_IGNORE(ignore, NO_MATCH, "yK");
  EXPECT_IGNORE(ignore, NO_MATCH, "yx");
  EXPECT_IGNORE(ignore, NO_MATCH, "yz");
  EXPECT_IGNORE(ignore, EXCLUDE, "z0");
  EXPECT_IGNORE(ignore, EXCLUDE, "z9");
  EXPECT_IGNORE(ignore, EXCLUDE, "z-");
  EXPECT_IGNORE(ignore, EXCLUDE, "zz");
  EXPECT_IGNORE(ignore, NO_MATCH, "zy");
  EXPECT_IGNORE(ignore, EXCLUDE, "0[]");
  EXPECT_IGNORE(ignore, EXCLUDE, "0:]");
  EXPECT_IGNORE(ignore, EXCLUDE, "0a]");
  EXPECT_IGNORE(ignore, EXCLUDE, "0p]");
  EXPECT_IGNORE(ignore, NO_MATCH, "0]]");
  EXPECT_IGNORE(ignore, NO_MATCH, "0a");

  // ] immediately after an opening [ is treated as part of the character class
  ignore.loadFile(
      "foo\n"
      "test[]x]test\n"
      "abc[!]x]def\n"
      "bar\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, EXCLUDE, "bar");
  EXPECT_IGNORE(ignore, EXCLUDE, "test]test");
  EXPECT_IGNORE(ignore, EXCLUDE, "testxtest");
  EXPECT_IGNORE(ignore, NO_MATCH, "test_test");
  EXPECT_IGNORE(ignore, NO_MATCH, "abcxdef");
  EXPECT_IGNORE(ignore, NO_MATCH, "abc]def");
  EXPECT_IGNORE(ignore, EXCLUDE, "abczdef");

  // Ensure bogus char class patterns are ignored
  ignore.loadFile(
      "pattern1\n"
      "foo[abc\n"
      "test\n");
  EXPECT_IGNORE(ignore, EXCLUDE, "pattern1");
  EXPECT_IGNORE(ignore, EXCLUDE, "test");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo");
  EXPECT_IGNORE(ignore, NO_MATCH, "fooa");
  EXPECT_IGNORE(ignore, NO_MATCH, "foo[abc");

  // Make sure the code handles an unterminated
  // character class expressions at the very end of the file.
  ignore.loadFile("bogus[pattern");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogus[p");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogus[pattern");
  ignore.loadFile("bogus[[:alpha");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  ignore.loadFile("bogus[[:alpha:");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  ignore.loadFile("bogus[[:");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  ignore.loadFile("bogus[[");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  ignore.loadFile("bogus[");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusp");
  ignore.loadFile("bogus[a-");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusa");
  ignore.loadFile("bogus[a");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusa");
  ignore.loadFile("bogus[-");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusa");
  ignore.loadFile("bogus[!");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusa");
  ignore.loadFile("bogus[^");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogus");
  ignore.loadFile("bogus[^a-");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogusX");
  ignore.loadFile("bogus[^-");
  EXPECT_IGNORE(ignore, NO_MATCH, "bogus-");
}

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

TEST(GitIgnore, directory) {
  GitIgnore ignore;
  ignore.loadFile(
      "junk/\n"
      "foo\n"
      "!bar\n"
      "/build/\n");

  EXPECT_IGNORE(ignore, NO_MATCH, "junk");
  EXPECT_IGNORE_DIR(ignore, EXCLUDE, "junk");
  EXPECT_IGNORE(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE_DIR(ignore, EXCLUDE, "foo");
  EXPECT_IGNORE(ignore, INCLUDE, "bar");
  EXPECT_IGNORE_DIR(ignore, INCLUDE, "bar");
  EXPECT_IGNORE(ignore, NO_MATCH, "build");
  EXPECT_IGNORE_DIR(ignore, EXCLUDE, "build");

  EXPECT_IGNORE(ignore, NO_MATCH, "test/junk");
  EXPECT_IGNORE_DIR(ignore, EXCLUDE, "test/junk");

  EXPECT_IGNORE_DIR(ignore, NO_MATCH, "test/build");
  EXPECT_IGNORE_DIR(ignore, INCLUDE, "test/build/bar");
  EXPECT_IGNORE_DIR(ignore, EXCLUDE, "test/build/foo");

  // Note: we intentionally do not include checks for files like
  // "test/junk/bar" and "build/bar".  The GitIgnoreStack code should always
  // stop when it finds an excluded directory, and should not descend into it
  // and try matching these patterns.  The results of these checks therefore do
  // not matter.
  //
  // In practice the results are potentially slightly unexpected, because
  // the GitIgnore code completely skips directory-only rules when processing a
  // path known to be a file.  It expects ignored directories earlier in the
  // path to have already been filtered out.
}
