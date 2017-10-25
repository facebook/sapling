/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/testharness/TestUtil.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(TreeEntry, modeAndLogString) {
  TreeEntry rwFile(
      makeTestHash("faceb00c"), "file.txt", FileType::REGULAR_FILE, 0b110);
  EXPECT_EQ(S_IFREG | S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH, rwFile.getMode());
  EXPECT_EQ(
      "(file.txt, 00000000000000000000000000000000faceb00c, frw-)",
      rwFile.toLogString());

  TreeEntry rFile(makeTestHash("7"), "file.txt", FileType::REGULAR_FILE, 0b100);
  EXPECT_EQ(S_IFREG | S_IRUSR | S_IRGRP | S_IROTH, rFile.getMode());
  EXPECT_EQ(
      "(file.txt, 0000000000000000000000000000000000000007, fr--)",
      rFile.toLogString());

  TreeEntry rwxFile(
      makeTestHash("789"), "file.exe", FileType::REGULAR_FILE, 0b111);
  EXPECT_EQ(
      S_IFREG | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      rwxFile.getMode());
  EXPECT_EQ(
      "(file.exe, 0000000000000000000000000000000000000789, frwx)",
      rwxFile.toLogString());

  TreeEntry nopermsFile(
      makeTestHash("13"), "secret.txt", FileType::REGULAR_FILE, 0b000);
  EXPECT_EQ(S_IFREG, nopermsFile.getMode());
  EXPECT_EQ(
      "(secret.txt, 0000000000000000000000000000000000000013, f---)",
      nopermsFile.toLogString());

  TreeEntry rwLink(makeTestHash("a"), "to-file.txt", FileType::SYMLINK, 0b110);
  EXPECT_EQ(S_IFLNK | S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH, rwLink.getMode());
  EXPECT_EQ(
      "(to-file.txt, 000000000000000000000000000000000000000a, lrw-)",
      rwLink.toLogString());

  TreeEntry rwxLink(makeTestHash("b"), "to-file.exe", FileType::SYMLINK, 0b111);
  EXPECT_EQ(
      S_IFLNK | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      rwxLink.getMode());
  EXPECT_EQ(
      "(to-file.exe, 000000000000000000000000000000000000000b, lrwx)",
      rwxLink.toLogString());

  TreeEntry directory(makeTestHash("abc"), "src", FileType::DIRECTORY, 0b111);
  EXPECT_EQ(
      S_IFDIR | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      directory.getMode());
  EXPECT_EQ(
      "(src, 0000000000000000000000000000000000000abc, drwx)",
      directory.toLogString());
}
