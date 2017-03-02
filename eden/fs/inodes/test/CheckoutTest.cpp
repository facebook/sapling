/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/Conv.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::string;

enum LoadBehavior {
  LOAD_NONE,
  // TODO: add LIST_PARENT and LIST_FILE, where we list the parent directory
  // to assign an inode number to these entries, but do not load them yet.
  LOAD_PARENT,
  LOAD_FILE
};

void testAddFile(
    folly::StringPiece newFilePath,
    LoadBehavior loadType,
    int perms = 0644) {
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test/test.c", "testy tests");
  TestMount testMount{builder1};

  // Prepare a second tree, by starting with builder1 then adding the new file
  auto builder2 = builder1.clone();
  builder2.setFile(newFilePath, "this is the new file contents\n", perms);
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  if (loadType != LOAD_NONE) {
    // Make sure the new path doesn't exist beforehand.
    // This mostly makes sure that the TreeInode for src gets loaded.
    EXPECT_THROW_ERRNO(testMount.getInode(newFilePath), ENOENT);
  }

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Confirm that the tree has been updated correctly.
  auto newInode = testMount.getFileInode(newFilePath);
  EXPECT_FILE_INODE(newInode, "this is the new file contents\n", perms);
}

void runAddFileTests(folly::StringPiece path) {
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(folly::to<string>("add ", path, " load type ", int(loadType)));
    testAddFile(path, loadType);
    testAddFile(path, loadType, 0444);
    testAddFile(path, loadType, 0755);
  }
}

TEST(Checkout, addFile) {
  // Test with file names that will be at the beginning of the directory,
  // in the middle of the directory, and at the end of the directory.
  // (The directory entries are processed in sorted order.)
  runAddFileTests("src/aaa.c");
  runAddFileTests("src/ppp.c");
  runAddFileTests("src/zzz.c");
}

void testRemoveFile(folly::StringPiece newFilePath, LoadBehavior loadType) {
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test/test.c", "testy tests");
  builder1.setFile(newFilePath, "this file will be removed\n");
  TestMount testMount{builder1};

  // Prepare a second tree, by starting with builder1 then removing the desired
  // file
  auto builder2 = builder1.clone();
  builder2.removeFile(newFilePath);
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  if (loadType == LOAD_FILE) {
    // Load the file inode and make sure its contents are as we expect.
    // This ensures the affected FileInode has been loaded
    auto fileInode = testMount.getFileInode(newFilePath);
    EXPECT_FILE_INODE(fileInode, "this file will be removed\n", 0644);
  } else if (loadType == LOAD_PARENT) {
    // Load the parent TreeInode but not the affected file
    testMount.getTreeInode(RelativePathPiece{newFilePath}.dirname());
  }

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Make sure the path doesn't exist any more.
  EXPECT_THROW_ERRNO(testMount.getInode(newFilePath), ENOENT);
}

void runRemoveFileTests(folly::StringPiece path) {
  // Modify just the file contents, but not the permissions
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(
        folly::to<string>("remove ", path, " load type ", int(loadType)));
    testRemoveFile(path, loadType);
  }
}

TEST(Checkout, removeFile) {
  // Test with file names that will be at the beginning of the directory,
  // in the middle of the directory, and at the end of the directory.
  // (The directory entries are processed in sorted order.)
  runRemoveFileTests("src/aaa.c");
  runRemoveFileTests("src/ppp.c");
  runRemoveFileTests("src/zzz.c");
}

void testModifyFile(
    folly::StringPiece path,
    LoadBehavior loadType,
    folly::StringPiece contents1,
    int perms1,
    folly::StringPiece contents2,
    int perms2) {
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("readme.txt", "just filling out the tree\n");
  builder1.setFile("a/test.txt", "test contents\n");
  builder1.setFile("a/b/dddd.c", "this is dddd.c\n");
  builder1.setFile("a/b/tttt.c", "this is tttt.c\n");
  builder1.setFile(path, contents1, perms1);
  TestMount testMount{builder1};

  // Prepare the second tree
  auto builder2 = builder1.clone();
  builder2.replaceFile(path, contents2, perms2);
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  if (loadType == LOAD_FILE) {
    // Load the FileInode and make sure its contents are as we expect.
    auto preInode = testMount.getFileInode(path);
    EXPECT_FILE_INODE(preInode, contents1, perms1);
  } else if (loadType == LOAD_PARENT) {
    // Load the parent TreeInode but not the affected file
    testMount.getTreeInode(RelativePathPiece{path}.dirname());
  }

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Make sure the path is updated as expected
  auto postInode = testMount.getFileInode(path);
  EXPECT_FILE_INODE(postInode, contents2, perms2);
}

void testModifyFile(
    folly::StringPiece path,
    LoadBehavior loadType,
    folly::StringPiece contents1,
    folly::StringPiece contents2) {
  testModifyFile(path, loadType, contents1, 0644, contents2, 0644);
}

void runModifyFileTests(folly::StringPiece path) {
  // Modify just the file contents, but not the permissions
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(folly::to<string>(
        "contents change, path ", path, " load type ", int(loadType)));
    testModifyFile(
        path, loadType, "contents v1", "updated file contents\nextra stuff\n");
  }

  // Modify just the permissions, but not the contents
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(folly::to<string>(
        "mode change, path ", path, " load type ", int(loadType)));
    testModifyFile(path, loadType, "unchanged", 0755, "unchanged", 0644);
  }

  // Modify the contents and the permissions
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(folly::to<string>(
        "contents+mode change, path ", path, " load type ", int(loadType)));
    testModifyFile(
        path, loadType, "contents v1", 0644, "executable contents", 0755);
  }
}

TEST(Checkout, modifyFile) {
  // Test with file names that will be at the beginning of the directory,
  // in the middle of the directory, and at the end of the directory.
  runModifyFileTests("a/b/aaa.txt");
  runModifyFileTests("a/b/mmm.txt");
  runModifyFileTests("a/b/zzz.txt");
}

void testModifyConflict(
    folly::StringPiece path,
    LoadBehavior loadType,
    bool force,
    folly::StringPiece contents1,
    int perms1,
    folly::StringPiece currentContents,
    int currentPerms,
    folly::StringPiece contents2,
    int perms2) {
  // Prepare the tree to represent the current inode state
  auto workingDirBuilder = FakeTreeBuilder();
  workingDirBuilder.setFile("readme.txt", "just filling out the tree\n");
  workingDirBuilder.setFile("a/test.txt", "test contents\n");
  workingDirBuilder.setFile("a/b/dddd.c", "this is dddd.c\n");
  workingDirBuilder.setFile("a/b/tttt.c", "this is tttt.c\n");
  workingDirBuilder.setFile(path, currentContents, currentPerms);
  TestMount testMount{workingDirBuilder};

  // Prepare the "before" tree
  auto builder1 = workingDirBuilder.clone();
  builder1.replaceFile(path, contents1, perms1);
  builder1.finalize(testMount.getBackingStore(), true);
  // Reset the EdenMount to point at the tree from builder1, even though the
  // contents are still from workingDirBuilder.  This lets us trigger the
  // desired conflicts.
  //
  // TODO: We should also do a test where we start from builder1 then use
  // EdenDispatcher APIs to modify the contents to the "current" state.
  // This will have a different behavior than when using
  // resetCommit(), as the files will be materialized this way.
  auto commit1 = testMount.getBackingStore()->putCommit("a", builder1);
  commit1->setReady();
  testMount.getEdenMount()->resetCommit(makeTestHash("a"));

  // Prepare the destination tree
  auto builder2 = builder1.clone();
  builder2.replaceFile(path, contents2, perms2);
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("b", builder2);
  commit2->setReady();

  if (loadType == LOAD_FILE) {
    // Load the FileInode and make sure its contents are as we expect.
    auto preInode = testMount.getFileInode(path);
    EXPECT_FILE_INODE(preInode, currentContents, currentPerms);
  } else if (loadType == LOAD_PARENT) {
    // Load the parent TreeInode but not the affected file
    testMount.getTreeInode(RelativePathPiece{path}.dirname());
  }

  auto checkoutResult =
      testMount.getEdenMount()->checkout(makeTestHash("b"), force);
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  ASSERT_EQ(1, results.size());

// We currently don't always report conflicts accurately.
//
// When we run into an unloaded, non-materialized directory with conflicts,
// we just replace it, and report the directory as a conflict, rather than
// loading it and  recursing down into it to find the exact file names with
// conflicts.
//
// TODO: We probably need to update the code to recurse down into the tree
// just to return an accurate conflict list in this case.
#if 0
  EXPECT_EQ(path, results[0].path);
#endif
  EXPECT_EQ(ConflictType::MODIFIED, results[0].type);

  auto postInode = testMount.getFileInode(path);
  if (force) {
    // Make sure the path is updated as expected
    EXPECT_FILE_INODE(postInode, contents2, perms2);
  } else {
    // Make sure the path has not been changed
    EXPECT_FILE_INODE(postInode, currentContents, currentPerms);
  }
}

void runModifyConflictTests(folly::StringPiece path) {
  for (auto loadType : {LOAD_NONE, LOAD_PARENT, LOAD_FILE}) {
    SCOPED_TRACE(folly::to<string>(
        "path ", path, " load type ", int(loadType), " force"));
    testModifyConflict(
        path,
        loadType,
        true,
        "orig file contents.txt",
        0644,
        "current file contents.txt",
        0644,
        "new file contents.txt",
        0644);
    SCOPED_TRACE(folly::to<string>(
        "path ", path, " load type ", int(loadType), " not force"));
    testModifyConflict(
        path,
        loadType,
        false,
        "orig file contents.txt",
        0644,
        "current file contents.txt",
        0644,
        "new file contents.txt",
        0644);
  }
}

TEST(Checkout, modifyConflict) {
  runModifyConflictTests("a/b/aaa.txt");
  runModifyConflictTests("a/b/mmm.txt");
  runModifyConflictTests("a/b/zzz.txt");
}

// TODO:
// - add sub directory
// - remove subdirectory
//   - with no untracked/ignored files, it should get removed entirely
//   - remove subdirectory with untracked files
// - add/modify/replace symlink
//
// - change file type:
//   regular -> directory
//   regular -> symlink
//   symlink -> regular
//   symlink -> directory
//   directory -> regular
//   - also with error due to untracked files in directory
//   directory -> symlink
//   - also with error due to untracked files in directory
//
// - conflict handling, with and without --clean
//   - modify file, with removed conflict
//   - modify file, with changed file type conflict
//   - modify file, with a parent directory replaced with a file/symlink
//   - add file, with untracked file/directory/symlink already there
//   - add file, with a parent directory replaced with a file/symlink
//   - remove file, with modify conflict
//   - remove file, with remove conflict
//   - remove file, with a parent directory replaced with a file/symlink
