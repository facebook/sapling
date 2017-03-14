/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/Array.h>
#include <folly/Conv.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestChecks.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::string;

/**
 * An enum to control behavior for many of the checkout tests.
 *
 * Whether or not inodes are loaded when checkout runs affects which code
 * paths we hit, but it should not affect the user-visible behavior.
 */
enum class LoadBehavior {
  // None of the inodes in question are explicitly loaded
  // before the checkout operation.
  NONE,
  // Assign an inode number for the parent directory, but do not load it yet.
  ASSIGN_PARENT_INODE,
  // Load the parent TreeInode object before starting the checkout.
  PARENT,
  // Load the parent TreeInode object, and assign an inode number to the
  // child in question, but do not load the child InodeBase.
  ASSIGN_INODE,
  // Load the InodeBase affected by the test before starting the checkout.
  INODE,
};

static constexpr auto kAllLoadTypes = folly::make_array(
    LoadBehavior::NONE,
    LoadBehavior::ASSIGN_PARENT_INODE,
    LoadBehavior::PARENT,
    LoadBehavior::ASSIGN_INODE,
    LoadBehavior::INODE);

// LoadTypes that can be used with tests that add a new file
static constexpr auto kAddLoadTypes = folly::make_array(
    LoadBehavior::NONE,
    LoadBehavior::ASSIGN_PARENT_INODE,
    LoadBehavior::PARENT);

std::ostream& operator<<(std::ostream& os, LoadBehavior loadType) {
  switch (loadType) {
    case LoadBehavior::NONE:
      os << "NONE";
      return os;
    case LoadBehavior::ASSIGN_PARENT_INODE:
      os << "ASSIGN_PARENT_INODE";
      return os;
    case LoadBehavior::PARENT:
      os << "PARENT";
      return os;
    case LoadBehavior::ASSIGN_INODE:
      os << "ASSIGN_INODE";
      return os;
    case LoadBehavior::INODE:
      os << "INODE";
      return os;
  }

  os << "<unknown LoadBehavior " << int(loadType) << ">";
  return os;
}

void loadInodes(
    TestMount& testMount,
    RelativePathPiece path,
    LoadBehavior loadType,
    folly::Optional<folly::StringPiece> expectedContents,
    mode_t expectedPerms) {
  switch (loadType) {
    case LoadBehavior::NONE:
      return;
    case LoadBehavior::ASSIGN_PARENT_INODE: {
      // Load the parent TreeInode but not the affected file
      testMount.getTreeInode(path.dirname());
      auto parentPath = path.dirname();
      auto grandparentInode = testMount.getTreeInode(parentPath.dirname());
      grandparentInode->getChildInodeNumber(parentPath.basename());
      return;
    }
    case LoadBehavior::PARENT:
      // Load the parent TreeInode but not the affected file
      testMount.getTreeInode(path.dirname());
      return;
    case LoadBehavior::ASSIGN_INODE: {
      auto parent = testMount.getTreeInode(path.dirname());
      parent->getChildInodeNumber(path.basename());
      return;
    }
    case LoadBehavior::INODE: {
      // Load the file inode and make sure its contents are as we expect.
      // This ensures the affected FileInode has been loaded
      auto fileInode = testMount.getFileInode(path);
      if (expectedContents.hasValue()) {
        EXPECT_FILE_INODE(fileInode, expectedContents.value(), expectedPerms);
      }
      return;
    }
  }

  FAIL() << "unknown load behavior: " << loadType;
}

void loadInodes(
    TestMount& testMount,
    folly::StringPiece path,
    LoadBehavior loadType,
    folly::StringPiece expectedContents,
    mode_t expectedPerms = 0644) {
  loadInodes(
      testMount,
      RelativePathPiece{path},
      loadType,
      expectedContents,
      expectedPerms);
}

void loadInodes(
    TestMount& testMount,
    folly::StringPiece path,
    LoadBehavior loadType) {
  loadInodes(testMount, RelativePathPiece{path}, loadType, folly::none, 0644);
}

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

  loadInodes(testMount, newFilePath, loadType);

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Confirm that the tree has been updated correctly.
  auto newInode = testMount.getFileInode(newFilePath);
  EXPECT_FILE_INODE(newInode, "this is the new file contents\n", perms);
}

void runAddFileTests(folly::StringPiece path) {
  for (auto loadType : kAddLoadTypes) {
    SCOPED_TRACE(folly::to<string>("add ", path, " load type ", loadType));
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

void testRemoveFile(folly::StringPiece filePath, LoadBehavior loadType) {
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test/test.c", "testy tests");
  builder1.setFile(filePath, "this file will be removed\n");
  TestMount testMount{builder1};

  // Prepare a second tree, by starting with builder1 then removing the desired
  // file
  auto builder2 = builder1.clone();
  builder2.removeFile(filePath);
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  loadInodes(testMount, filePath, loadType, "this file will be removed\n");

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Make sure the path doesn't exist any more.
  EXPECT_THROW_ERRNO(testMount.getInode(filePath), ENOENT);
}

void runRemoveFileTests(folly::StringPiece path) {
  // Modify just the file contents, but not the permissions
  for (auto loadType : kAllLoadTypes) {
    SCOPED_TRACE(folly::to<string>("remove ", path, " load type ", loadType));
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

  loadInodes(testMount, path, loadType, contents1, perms1);

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
  for (auto loadType : kAllLoadTypes) {
    SCOPED_TRACE(folly::to<string>(
        "contents change, path ", path, " load type ", loadType));
    testModifyFile(
        path, loadType, "contents v1", "updated file contents\nextra stuff\n");
  }

  // Modify just the permissions, but not the contents
  for (auto loadType : kAllLoadTypes) {
    SCOPED_TRACE(
        folly::to<string>("mode change, path ", path, " load type ", loadType));
    testModifyFile(path, loadType, "unchanged", 0755, "unchanged", 0644);
  }

  // Modify the contents and the permissions
  for (auto loadType : kAllLoadTypes) {
    SCOPED_TRACE(folly::to<string>(
        "contents+mode change, path ", path, " load type ", loadType));
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

  loadInodes(testMount, path, loadType, currentContents, currentPerms);

  auto checkoutResult =
      testMount.getEdenMount()->checkout(makeTestHash("b"), force);
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  ASSERT_EQ(1, results.size());

  EXPECT_EQ(path, results[0].path);
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
  for (auto loadType : kAllLoadTypes) {
    for (bool force : {true, false}) {
      SCOPED_TRACE(folly::to<string>(
          "path ", path, " load type ", loadType, " force=", force));
      testModifyConflict(
          path,
          loadType,
          force,
          "orig file contents.txt",
          0644,
          "current file contents.txt",
          0644,
          "new file contents.txt",
          0644);
    }
  }
}

TEST(Checkout, modifyConflict) {
  runModifyConflictTests("a/b/aaa.txt");
  runModifyConflictTests("a/b/mmm.txt");
  runModifyConflictTests("a/b/zzz.txt");
}

void testAddSubdirectory(folly::StringPiece newDirPath, LoadBehavior loadType) {
  auto builder1 = FakeTreeBuilder();
  builder1.setFile("src/main.c", "int main() { return 0; }\n");
  builder1.setFile("src/test/test.c", "testy tests");
  TestMount testMount{builder1};

  // Prepare a second tree, by starting with builder1 then adding
  // the new directory
  auto builder2 = builder1.clone();
  RelativePathPiece newDir{newDirPath};
  builder2.setFile(newDir + PathComponentPiece{"doc.txt"}, "docs\n");
  builder2.setFile(newDir + PathComponentPiece{"file1.c"}, "src\n");
  builder2.setFile(newDir + RelativePathPiece{"include/file1.h"}, "header\n");
  builder2.finalize(testMount.getBackingStore(), true);
  auto commit2 = testMount.getBackingStore()->putCommit("2", builder2);
  commit2->setReady();

  loadInodes(testMount, newDirPath, loadType);

  auto checkoutResult = testMount.getEdenMount()->checkout(makeTestHash("2"));
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Confirm that the tree has been updated correctly.
  EXPECT_FILE_INODE(
      testMount.getFileInode(newDir + PathComponentPiece{"doc.txt"}),
      "docs\n",
      0644);
  EXPECT_FILE_INODE(
      testMount.getFileInode(newDir + PathComponentPiece{"file1.c"}),
      "src\n",
      0644);
  EXPECT_FILE_INODE(
      testMount.getFileInode(newDir + RelativePathPiece{"include/file1.h"}),
      "header\n",
      0644);
}

TEST(Checkout, addSubdirectory) {
  // Test with multiple paths to exercise the case where the modification is at
  // the start of the directory listing, at the end, and in the middle.
  for (const auto& path : {"src/aaa", "src/ppp", "src/zzz"}) {
    for (auto loadType : kAddLoadTypes) {
      SCOPED_TRACE(folly::to<string>("path ", path, " load type ", loadType));
      testAddSubdirectory(path, loadType);
    }
  }
}

// TODO:
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
