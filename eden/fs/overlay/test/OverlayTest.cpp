/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/FileUtil.h>
#include <folly/experimental/TestUtil.h>
#include <gtest/gtest.h>
#include <system_error>
#include "eden/fs/overlay/Overlay.h"
#include "eden/utils/DirType.h"
#include "eden/utils/PathFuncs.h"

using namespace facebook::eden;
using TempDir = folly::test::TemporaryDirectory;

TEST(Overlay, empty) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  auto top_contents = overlay.readDir(RelativePathPiece());
  EXPECT_EQ(0, top_contents.entries.size()) << "No content to start with";
  EXPECT_FALSE(top_contents.isOpaque);
}

TEST(Overlay, removeNonExistentFile) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.removeFile(RelativePathPiece("nosuchfile.txt"), false);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(0, contents.entries.size());
}

TEST(Overlay, removeNonExistentDir) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.removeDir(RelativePathPiece("nodir"), false);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(0, contents.entries.size());
}

TEST(Overlay, makeFile) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  // Create a file in the overlay.
  {
    auto f = overlay.openFile(
        RelativePathPiece("foo.txt"), O_CREAT | O_WRONLY, 0600);
    folly::writeNoInt(f.fd(), "hello\n", 6);
    f.close();
  }

  // Let's ensure that we observe it at the correct location in the filesystem.
  std::string content;
  EXPECT_EQ(
      true,
      folly::readFile(
          (AbsolutePathPiece(localDir.path().string()) +
           PathComponentPiece("foo.txt"))
              .c_str(),
          content));
  EXPECT_EQ("hello\n", content)
      << "file is in the correct place and has the correct contents";

  // and that it shows up in the contents.
  auto top_contents = overlay.readDir(RelativePathPiece());
  EXPECT_EQ(1, top_contents.entries.size()) << "1 entry";
  EXPECT_FALSE(top_contents.isOpaque);
  EXPECT_EQ(dtype_t::Regular, top_contents.entries[PathComponent("foo.txt")])
      << "regular file foo.txt";
  EXPECT_FALSE(top_contents.isOpaque);
}

// Check that we can build out the directory structure.
TEST(Overlay, mkdirs) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.makeDir(RelativePathPiece("build/me/out"), 0700);

  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("build")]);

  contents = overlay.readDir(RelativePathPiece("build"));
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("me")]);

  contents = overlay.readDir(RelativePathPiece("build/me"));
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("out")]);
}

TEST(Overlay, removeDirEmpty) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.removeDir(RelativePathPiece("nothere"), true);

  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Whiteout, contents.entries[PathComponent("nothere")]);
}

TEST(Overlay, mkdirsWhiteout) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.removeDir(RelativePathPiece("nothere"), true);
  // can't build out a tree under a whiteout node unless you explicit mkdir the
  // root of it.
  EXPECT_THROW(
      overlay.makeDir(RelativePathPiece("nothere/sub/dir"), 0700),
      std::system_error);

  // similarly for files.
  EXPECT_THROW(
      overlay.openFile(
          RelativePathPiece("nothere/foo.txt"), O_CREAT | O_RDWR, 0600),
      std::system_error);
}

TEST(Overlay, removeFileWhiteout) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.openFile(RelativePathPiece("foo"), O_CREAT | O_RDWR, 0600);

  overlay.removeFile(RelativePathPiece("foo"), true);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Whiteout, contents.entries[PathComponent("foo")]);

  struct stat st;
  EXPECT_EQ(
      -1,
      lstat(
          (AbsolutePathPiece(localDir.path().string()) +
           PathComponentPiece("foo"))
              .c_str(),
          &st));
  EXPECT_EQ(ENOENT, errno);
}

TEST(Overlay, removeFileNoWhiteout) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.openFile(RelativePathPiece("foo"), O_CREAT | O_RDWR, 0600);

  overlay.removeFile(RelativePathPiece("foo"), false);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(0, contents.entries.size());
}

TEST(Overlay, removeFileWhiteoutAndRecreate) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.openFile(RelativePathPiece("foo"), O_CREAT | O_RDWR, 0600);

  overlay.removeFile(RelativePathPiece("foo"), true);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Whiteout, contents.entries[PathComponent("foo")]);

  overlay.openFile(RelativePathPiece("foo"), O_CREAT | O_RDWR, 0600);
  contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Regular, contents.entries[PathComponent("foo")]);
}

TEST(Overlay, removeDir) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.makeDir(RelativePathPiece("top"), 0700);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("top")]);

  overlay.removeDir(RelativePathPiece("top"), false);
  contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(0, contents.entries.size());
}

TEST(Overlay, removeDirWhiteout) {
  TempDir localDir;
  Overlay overlay(AbsolutePathPiece(localDir.path().string()));

  overlay.makeDir(RelativePathPiece("top"), 0700);
  auto contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("top")]);

  overlay.removeDir(RelativePathPiece("top"), true);
  contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Whiteout, contents.entries[PathComponent("top")]);

  overlay.makeDir(RelativePathPiece("top"), 0700);
  contents = overlay.readDir(RelativePathPiece());
  EXPECT_FALSE(contents.isOpaque);
  EXPECT_EQ(1, contents.entries.size());
  EXPECT_EQ(dtype_t::Dir, contents.entries[PathComponent("top")]);

  contents = overlay.readDir(RelativePathPiece("top"));
  EXPECT_TRUE(contents.isOpaque) << "replaced dir is opaque";
  EXPECT_EQ(0, contents.entries.size());
}
