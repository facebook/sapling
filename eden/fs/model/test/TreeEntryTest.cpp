/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/model/TreeEntry.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(TreeEntry, getMode) {
  TreeEntry rwFile(Hash(), "file.txt", FileType::REGULAR_FILE, 0b0110);
  EXPECT_EQ(S_IFREG | S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH, rwFile.getMode());

  TreeEntry rFile(Hash(), "file.txt", FileType::REGULAR_FILE, 0b0100);
  EXPECT_EQ(S_IFREG | S_IRUSR | S_IRGRP | S_IROTH, rFile.getMode());

  TreeEntry rwxFile(Hash(), "file.exe", FileType::REGULAR_FILE, 0b0111);
  EXPECT_EQ(
      S_IFREG | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      rwxFile.getMode());

  TreeEntry rwLink(Hash(), "to-file.txt", FileType::SYMLINK, 0b0110);
  EXPECT_EQ(S_IFLNK | S_IRUSR | S_IWUSR | S_IRGRP | S_IROTH, rwLink.getMode());

  TreeEntry rwxLink(Hash(), "to-file.exe", FileType::SYMLINK, 0b0111);
  EXPECT_EQ(
      S_IFLNK | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      rwxLink.getMode());

  TreeEntry directory(Hash(), "src", FileType::DIRECTORY, 0b0111);
  EXPECT_EQ(
      S_IFDIR | S_IRWXU | S_IRGRP | S_IXGRP | S_IROTH | S_IXOTH,
      directory.getMode());
}
