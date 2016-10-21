/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/service/EdenMountHandler.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/utils/PathFuncs.h"

#include <gtest/gtest.h>

using namespace facebook::eden;

TEST(EdenMountHandler, getModifiedDirectoriesForMountWithNoModifications) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto edenMount = testMount->getEdenMount();
  auto modifiedDirectories = getModifiedDirectoriesForMount(edenMount.get());
  std::vector<RelativePath> expected = {};
  EXPECT_EQ(expected, *modifiedDirectories.get());
}

TEST(EdenMountHandler, getModifiedDirectoriesForMount) {
  TestMountBuilder builder;
  builder.addFiles({
      {"animals/c/cat", "meow"}, {"animals/d/dog", "woof"},
  });
  auto testMount = builder.build();

  testMount->mkdir("x");
  testMount->mkdir("x/y");
  testMount->mkdir("x/y/z");
  testMount->addFile("x/file.txt", "");
  testMount->addFile("x/y/file.txt", "");
  testMount->addFile("x/y/z/file.txt", "");

  testMount->addFile("animals/c/cow", "moo");

  testMount->mkdir("a");
  testMount->mkdir("a/b");
  testMount->mkdir("a/b/c");
  testMount->addFile("a/file.txt", "");
  testMount->addFile("a/b/file.txt", "");
  testMount->addFile("a/b/c/file.txt", "");

  auto edenMount = testMount->getEdenMount();
  auto modifiedDirectories = getModifiedDirectoriesForMount(edenMount.get());

  std::vector<RelativePath> expected = {
      RelativePath(),
      RelativePath("a"),
      RelativePath("a/b"),
      RelativePath("a/b/c"),
      RelativePath("animals"),
      RelativePath("animals/c"),
      RelativePath("x"),
      RelativePath("x/y"),
      RelativePath("x/y/z"),
  };
  EXPECT_EQ(expected, *modifiedDirectories.get());
}
