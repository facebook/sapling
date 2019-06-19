/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/Format.h>
#include <folly/String.h>
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/Bug.h"

using namespace std::chrono_literals;
using namespace facebook::eden;
using folly::StringPiece;

class RenameTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    FakeTreeBuilder builder;
    builder.setFiles({
        {"a/b/c/doc.txt", "This file is used for most of the file renames.\n"},
        {"a/readme.txt", "I exist to be replaced.\n"},
        {"a/b/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/d/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/d/e/f/readme.txt", "I exist to be replaced.\n"},
        {"a/x/y/z/readme.txt", "I exist to be replaced.\n"},
    });
    mount_ = std::make_unique<TestMount>(builder);
    // Also create some empty directories for the tests
    mount_->mkdir("a/emptydir");
    mount_->mkdir("a/b/emptydir");
    mount_->mkdir("a/b/c/emptydir");
    mount_->mkdir("a/b/c/d/emptydir");
    mount_->mkdir("a/b/c/d/e/f/emptydir");
    mount_->mkdir("a/x/y/z/emptydir");
    mount_->mkdir("a/b/c/1");
    mount_->mkdir("a/b/c/1/2");
    mount_->mkdir("a/b/c/1/emptydir");
    mount_->mkdir("a/b/c/1/2/emptydir");
  }

  void
  renameFile(StringPiece srcPathStr, StringPiece destPathStr, bool destExists);
  void
  renameDir(StringPiece srcPathStr, StringPiece destPathStr, bool destExists);
  void renameError(
      StringPiece srcPathStr,
      StringPiece destPathStr,
      int expectedError);

  std::unique_ptr<TestMount> mount_;
};

/*
 * Basic tests for renaming files
 */

void RenameTest::renameFile(
    StringPiece srcPathStr,
    StringPiece destPathStr,
    bool destExists) {
  RelativePath srcPath{srcPathStr};
  auto srcBase = srcPath.basename();
  RelativePath destPath{destPathStr};
  auto destBase = destPath.basename();

  // Get the file pre-rename
  auto origSrc = mount_->getFileInode(srcPath);
  EXPECT_EQ(srcPath, origSrc->getPath().value());
  FileInodePtr origDest;
  if (destExists) {
    origDest = mount_->getFileInode(destPath);
    EXPECT_EQ(destPath, origDest->getPath().value());
    EXPECT_NE(origSrc->getNodeId(), origDest->getNodeId());
  } else {
    EXPECT_THROW_ERRNO(mount_->getFileInode(destPath), ENOENT);
  }

  // Do the rename
  auto srcDir = mount_->getTreeInode(srcPath.dirname());
  auto destDir = mount_->getTreeInode(destPath.dirname());
  auto renameFuture = srcDir->rename(srcBase, destDir, destBase);
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // Now get the file post-rename
  // Make sure it is the same inode, but the path is updated
  auto renamedInode = mount_->getFileInode(destPath);
  EXPECT_EQ(destPath, renamedInode->getPath().value());
  EXPECT_EQ(origSrc->getNodeId(), renamedInode->getNodeId());
  EXPECT_EQ(origSrc.get(), renamedInode.get());
  EXPECT_EQ(destPath, origSrc->getPath().value());

  // The original test file should now be unlinked
  if (destExists) {
    EXPECT_TRUE(origDest->isUnlinked());
  }

  // Trying to access the original name now should fail
  EXPECT_THROW_ERRNO(mount_->getFileInode(srcPath), ENOENT);
}

TEST_F(RenameTest, renameFileSameDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/newdocs.txt", false);
}

TEST_F(RenameTest, renameFileParentDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/newdocs.txt", false);
}

TEST_F(RenameTest, renameFileChildDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/d/newdocs.txt", false);
}

TEST_F(RenameTest, renameFileAncestorDirectory) {
  renameFile("a/b/c/doc.txt", "a/newdocs.txt", false);
}

TEST_F(RenameTest, renameFileDescendantDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/d/e/f/newdocs.txt", false);
}

TEST_F(RenameTest, renameFileOtherDirectory) {
  renameFile("a/b/c/doc.txt", "a/x/y/z/newdocs.txt", false);
}

TEST_F(RenameTest, replaceFileSameDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/readme.txt", true);
}

TEST_F(RenameTest, replaceFileParentDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/readme.txt", true);
}

TEST_F(RenameTest, replaceFileChildDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/d/readme.txt", true);
}

TEST_F(RenameTest, replaceFileAncestorDirectory) {
  renameFile("a/b/c/doc.txt", "a/readme.txt", true);
}

TEST_F(RenameTest, replaceFileDescendantDirectory) {
  renameFile("a/b/c/doc.txt", "a/b/c/d/e/f/readme.txt", true);
}

TEST_F(RenameTest, replaceFileOtherDirectory) {
  renameFile("a/b/c/doc.txt", "a/x/y/z/readme.txt", true);
}

TEST_F(RenameTest, renameFileToSamePath) {
  RelativePath path{"a/b/c/doc.txt"};

  // Get the file pre-rename
  auto origFile = mount_->getFileInode(path);
  EXPECT_EQ(path, origFile->getPath().value());

  // Do the rename
  auto parentDir = mount_->getTreeInode(path.dirname());
  auto renameFuture =
      parentDir->rename(path.basename(), parentDir, path.basename());
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // Just to be thorough, make sure looking up the path still returns the
  // original inode.
  auto renamedInode = mount_->getFileInode(path);
  EXPECT_EQ(path, renamedInode->getPath().value());
  EXPECT_EQ(origFile->getNodeId(), renamedInode->getNodeId());
  EXPECT_EQ(origFile.get(), renamedInode.get());
  EXPECT_EQ(path, origFile->getPath().value());
}

/*
 * Basic tests for renaming directories
 */

void RenameTest::renameDir(
    StringPiece srcPathStr,
    StringPiece destPathStr,
    bool destExists) {
  RelativePath srcPath{srcPathStr};
  auto srcBase = srcPath.basename();
  RelativePath destPath{destPathStr};
  auto destBase = destPath.basename();

  // Get the trees pre-rename
  auto origSrc = mount_->getTreeInode(srcPath);
  EXPECT_EQ(srcPath, origSrc->getPath().value());
  TreeInodePtr origDest;
  if (destExists) {
    origDest = mount_->getTreeInode(destPath);
    EXPECT_EQ(destPath, origDest->getPath().value());
    EXPECT_NE(origSrc->getNodeId(), origDest->getNodeId());
  } else {
    EXPECT_THROW_ERRNO(mount_->getTreeInode(destPath), ENOENT);
  }

  // Do the rename
  auto srcDir = mount_->getTreeInode(srcPath.dirname());
  auto destDir = mount_->getTreeInode(destPath.dirname());
  auto renameFuture = srcDir->rename(srcBase, destDir, destBase);
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // Now get the file post-rename
  // Make sure it is the same inode, but the path is updated
  auto renamedInode = mount_->getTreeInode(destPath);
  EXPECT_EQ(destPath, renamedInode->getPath().value());
  EXPECT_EQ(origSrc->getNodeId(), renamedInode->getNodeId());
  EXPECT_EQ(origSrc.get(), renamedInode.get());
  EXPECT_EQ(destPath, origSrc->getPath().value());

  // The original test file should now be unlinked
  if (destExists) {
    EXPECT_TRUE(origDest->isUnlinked());
  }

  // Trying to access the original name now should fail
  EXPECT_THROW_ERRNO(mount_->getTreeInode(srcPath), ENOENT);
}

TEST_F(RenameTest, renameDirSameDirectory) {
  renameDir("a/b/c/d", "a/b/c/newdir", false);
}

TEST_F(RenameTest, renameDirParentDirectory) {
  renameDir("a/b/c/d", "a/b/newdir", false);
}

TEST_F(RenameTest, renameDirChildDirectory) {
  renameDir("a/b/c/d", "a/b/c/1/newdir", false);
}

TEST_F(RenameTest, renameDirAncestorDirectory) {
  renameDir("a/b/c/d", "a/newdir", false);
}

TEST_F(RenameTest, renameDirDescendantDirectory) {
  renameDir("a/b/c/d", "a/b/c/1/2/newdir", false);
}

TEST_F(RenameTest, renameDirOtherDirectory) {
  renameDir("a/b/c/d", "a/x/y/z/newdir", false);
}

TEST_F(RenameTest, replaceDirSameDirectory) {
  renameDir("a/b/c/d", "a/b/c/emptydir", true);
}

TEST_F(RenameTest, replaceDirParentDirectory) {
  renameDir("a/b/c/d", "a/b/emptydir", true);
}

TEST_F(RenameTest, replaceDirChildDirectory) {
  renameDir("a/b/c/d", "a/b/c/1/emptydir", true);
}

TEST_F(RenameTest, replaceDirAncestorDirectory) {
  renameDir("a/b/c/d", "a/emptydir", true);
}

TEST_F(RenameTest, replaceDirDescendantDirectory) {
  renameDir("a/b/c/d", "a/b/c/1/2/emptydir", true);
}

TEST_F(RenameTest, replaceDirOtherDirectory) {
  renameDir("a/b/c/d", "a/x/y/z/emptydir", true);
}

TEST_F(RenameTest, renameDirToSamePath) {
  RelativePath path{"a/b/c/d"};

  // Get the file pre-rename
  auto origDir = mount_->getTreeInode(path);
  EXPECT_EQ(path, origDir->getPath().value());

  // Do the rename
  auto parentDir = mount_->getTreeInode(path.dirname());
  auto renameFuture =
      parentDir->rename(path.basename(), parentDir, path.basename());
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // Just to be thorough, make sure looking up the path still returns the
  // original inode.
  auto renamedInode = mount_->getTreeInode(path);
  EXPECT_EQ(path, renamedInode->getPath().value());
  EXPECT_EQ(origDir->getNodeId(), renamedInode->getNodeId());
  EXPECT_EQ(origDir.get(), renamedInode.get());
  EXPECT_EQ(path, origDir->getPath().value());
}

/*
 * Tests for error conditions
 */

void RenameTest::renameError(
    StringPiece srcPathStr,
    StringPiece destPathStr,
    int expectedError) {
  RelativePath srcPath{srcPathStr};
  auto srcBase = srcPath.basename();
  RelativePath destPath{destPathStr};
  auto destBase = destPath.basename();

  // Do the rename
  auto srcDir = mount_->getTreeInode(srcPath.dirname());
  auto destDir = mount_->getTreeInode(destPath.dirname());
  auto renameFuture = srcDir->rename(srcBase, destDir, destBase);

  // The rename should fail with the expected error
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(std::move(renameFuture).get(), expectedError);
}

TEST_F(RenameTest, renameNonexistentFile) {
  renameError("a/b/c/foo.txt", "a/b/c/bar.txt", ENOENT);
}

TEST_F(RenameTest, renameFileOverEmptyDir) {
  renameError("a/b/c/doc.txt", "a/b/c/emptydir", EISDIR);
}

TEST_F(RenameTest, renameFileOverNonEmptyDir) {
  // For now we require EISDIR, although ENOTEMPTY also seems like it might be
  // potentially acceptable.
  renameError("a/b/c/doc.txt", "a/b/c/d", EISDIR);
}

TEST_F(RenameTest, renameDirOverFile) {
  renameError("a/b/c/d", "a/b/c/doc.txt", ENOTDIR);
}

TEST_F(RenameTest, renameDirOverNonEmptyDir) {
  renameError("a/b/c/1", "a/b/c/d", ENOTEMPTY);
}

/*
 * Several tests for invalid rename paths.
 * The linux kernel should make sure that invalid rename requests like
 * this don't make it to us via FUSE, but check to make sure our code
 * conservatively handles these errors anyway.
 */

TEST_F(RenameTest, renameToInvalidChildPath) {
  renameError("a/b/c/d", "a/b/c/d/newdir", EINVAL);
}

TEST_F(RenameTest, renameToInvalidDescendentPath) {
  renameError("a/b/c/d", "a/b/c/d/e/newdir", EINVAL);
}

TEST_F(RenameTest, renameToInvalidParentPath) {
  renameError("a/b/c/d", "a/b/c", ENOTEMPTY);
}

TEST_F(RenameTest, renameToInvalidAncestorPath) {
  renameError("a/b/c/d", "a/b", ENOTEMPTY);
}

TEST_F(RenameTest, renameIntoUnlinkedDir) {
  RelativePath srcPath{"a/b/c/doc.txt"};
  RelativePath destDirPath{"a/b/c/emptydir"};

  // Look up the source and destination directories
  auto srcDir = mount_->getTreeInode(srcPath.dirname());
  auto destDir = mount_->getTreeInode(destDirPath);

  // Now unlink the destination directory
  auto destDirParent = mount_->getTreeInode(destDirPath.dirname());
  auto rmdirFuture = destDirParent->rmdir(destDirPath.basename());
  ASSERT_TRUE(rmdirFuture.isReady());
  EXPECT_FALSE(rmdirFuture.hasException());
  std::move(rmdirFuture).get();

  // Do the rename
  auto renameFuture =
      srcDir->rename(srcPath.basename(), destDir, "test.txt"_pc);

  // The rename should fail with ENOENT since the destination directory no
  // longer exists
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(std::move(renameFuture).get(), ENOENT);
}

TEST_F(RenameTest, renameOverEmptyDir) {
  // Git and Mercurial can't represent empty trees, so use one of the
  // (materialized) empty directories.
  auto root = mount_->getRootTree();

  auto x = mount_->getTreeInode("a/x");
  auto yino = x->getChildInodeNumber("y"_pc);
  auto newParent = mount_->getTreeInode("a/b");

  (void)x->rename("y"_pc, newParent, "emptydir"_pc).get(0ms);

  EXPECT_EQ(yino, newParent->getChildInodeNumber("emptydir"_pc));
}

TEST_F(RenameTest, renameOverEmptyDirWithPositiveFuseRefcount) {
  // Git and Mercurial can't represent empty trees, so use one of the
  // (materialized) empty directories.
  auto root = mount_->getRootTree();

  auto x = mount_->getTreeInode("a/x");
  auto y = x->getOrLoadChildTree("y"_pc).get(0ms);
  auto yino = y->getNodeId();
  auto newParent = mount_->getTreeInode("a/b");
  auto toBeUnlinked = newParent->getOrLoadChildTree("emptydir"_pc).get(0ms);
  toBeUnlinked->incFuseRefcount();
  toBeUnlinked.reset();

  (void)x->rename("y"_pc, newParent, "emptydir"_pc).get(0ms);

  EXPECT_EQ(yino, newParent->getChildInodeNumber("emptydir"_pc));
}

TEST_F(RenameTest, renameUpdatesMtime) {
  auto bInode = mount_->getTreeInode("a/b");
  auto cInode = mount_->getTreeInode("a/b/c");

  EXPECT_EQ(
      mount_->getClock().getRealtime(), bInode->getMetadata().timestamps.mtime);
  EXPECT_EQ(
      mount_->getClock().getRealtime(), cInode->getMetadata().timestamps.mtime);

  mount_->getClock().advance(1s);

  auto renameFuture = cInode->rename(
      PathComponentPiece{"doc.txt"}, bInode, PathComponentPiece{"doc.txt"});
  EXPECT_TRUE(renameFuture.isReady());

  EXPECT_EQ(
      mount_->getClock().getRealtime(), bInode->getMetadata().timestamps.mtime);
  EXPECT_EQ(
      mount_->getClock().getRealtime(), cInode->getMetadata().timestamps.mtime);
}

/*
 * Rename tests where the source and destination inode objects
 * are not loaded yet when the rename starts.
 */
class RenameLoadingTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFiles({
        {"a/b/c/doc.txt", "documentation\n"},
        {"a/b/c/readme.txt", "more docs\n"},
        {"a/b/testdir/sample.txt", "Lorem ipsum dolor sit amet\n"},
    });
    builder_.mkdir("a/b/empty");
    mount_ = std::make_unique<TestMount>(builder_, false);
  }

  FakeTreeBuilder builder_;
  std::unique_ptr<TestMount> mount_;
};

TEST_F(RenameLoadingTest, renameDirSameDirectory) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename where the child inode ("a/b/c" in this case)
  // is not ready yet, because the data is not available from the BackingStore.
  //
  // For now we have to test this with a directory, and not a regular file,
  // since file inodes can always be loaded immediately (as long as their
  // parent inode is ready).  File inodes do not wait to load the blob data
  // from the backing store before creating the FileInode object.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "x"_pc);
  // The rename will not complete until a/b/c becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Now make a/b/c ready
  builder_.setReady("a/b/c");
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();
}

TEST_F(RenameLoadingTest, renameWithLoadPending) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Start a lookup on a/b/c before we start the rename
  auto inodeFuture = mount_->getEdenMount()->getInode("a/b/c"_relpath);
  EXPECT_FALSE(inodeFuture.isReady());

  // Perform a rename on a/b/c before that inode is ready.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "x"_pc);
  // The rename will not complete until a/b/c becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Now make a/b/c ready
  builder_.setReady("a/b/c");

  // Both the load and the rename should have completed
  ASSERT_TRUE(inodeFuture.isReady());
  ASSERT_TRUE(renameFuture.isReady());

  // The rename should be successful
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // From an API guarantee point of view, it would be fine for the load
  // to succeed or to fail with ENOENT here, since it was happening
  // concurrently with a rename() that moved the file away from the path we
  // requested.
  //
  // In practice our code currently always succeeds the load attempt.
  if (inodeFuture.hasException()) {
    EXPECT_THROW_ERRNO(std::move(inodeFuture).get(), ENOENT);
  } else {
    auto cInode = std::move(inodeFuture).get();
    EXPECT_EQ("a/b/x", cInode->getPath().value().stringPiece());
  }
}

TEST_F(RenameLoadingTest, loadWithRenamePending) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on a/b/c before that inode is ready.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "x"_pc);
  // The rename will not complete until a/b/c becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Also start a lookup on a/b/c after starting the rename
  auto inodeFuture = mount_->getEdenMount()->getInode("a/b/c"_relpath);
  EXPECT_FALSE(inodeFuture.isReady());

  // Now make a/b/c ready
  builder_.setReady("a/b/c");

  // Both the load and the rename should have completed
  ASSERT_TRUE(inodeFuture.isReady());
  ASSERT_TRUE(renameFuture.isReady());

  // The rename should be successful
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();

  // From an API guarantee point of view, it would be fine for the load
  // to succeed or to fail with ENOENT here, since it was happening
  // concurrently with a rename() that moved the file away from the path we
  // requested.
  //
  // In practice our code currently always succeeds the load attempt.
  if (inodeFuture.hasException()) {
    EXPECT_THROW_ERRNO(std::move(inodeFuture).get(), ENOENT);
  } else {
    auto cInode = std::move(inodeFuture).get();
    EXPECT_EQ("a/b/x", cInode->getPath().value().stringPiece());
  }
}

TEST_F(RenameLoadingTest, renameLoadFailure) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "x"_pc);
  // The rename will not complete until a/b/c becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Fail the load of a/b/c
  builder_.triggerError("a/b/c", std::domain_error("fake error for testing"));
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      std::move(renameFuture).get(),
      std::domain_error,
      "fake error for testing");
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDest) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "empty"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a/b/c ready first
  builder_.setReady("a/b/c");
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a/b/empty ready
  builder_.setReady("a/b/empty");

  // Both the load and the rename should have completed
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();
}

TEST_F(RenameLoadingTest, renameLoadDestOtherOrder) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "empty"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a/b/empty ready first
  builder_.setReady("a/b/empty");
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a/b/c ready
  builder_.setReady("a/b/c");

  // Both the load and the rename should have completed
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  std::move(renameFuture).get();
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDestNonempty) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "testdir"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a/b/c ready first
  builder_.setReady("a/b/c");
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a/b/testdir ready
  builder_.setReady("a/b/testdir");

  // The load should fail with ENOTEMPTY
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(std::move(renameFuture).get(), ENOTEMPTY);
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDestNonemptyOtherOrder) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "testdir"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a/b/testdir ready first.
  builder_.setReady("a/b/testdir");
  // The rename could potentially fail now, but it is also be fine for it to
  // wait for the source directory to be ready too before it performs
  // validation.  Therefore go ahead and make the source directory ready too
  // without checking renameFuture.isReady()
  builder_.setReady("a/b/c");

  // The load should fail with ENOTEMPTY
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(std::move(renameFuture).get(), ENOTEMPTY);
}

TEST_F(RenameLoadingTest, renameLoadDestFailure) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "empty"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a/b/c ready first
  builder_.setReady("a/b/c");
  EXPECT_FALSE(renameFuture.isReady());
  // Now fail the load on a/b/empty
  builder_.triggerError(
      "a/b/empty", std::domain_error("fake error for testing"));

  // Verify the rename failure
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      std::move(renameFuture).get(),
      std::domain_error,
      "fake error for testing");
}

TEST_F(RenameLoadingTest, renameLoadDestFailureOtherOrder) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "empty"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Fail the load on a/b/empty first
  builder_.triggerError(
      "a/b/empty", std::domain_error("fake error for testing"));
  // The rename may fail immediately, but it's also fine for it to wait
  // for the source load to finish too.  Therefore go ahead and finish the load
  // on a/b/c without checking renameFuture.isReady()
  builder_.setReady("a/b/c");

  // Verify the rename failure
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      std::move(renameFuture).get(),
      std::domain_error,
      "fake error for testing");
}

TEST_F(RenameLoadingTest, renameLoadBothFailure) {
  builder_.setReady("a");
  builder_.setReady("a/b");

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename("c"_pc, bInode, "empty"_pc);
  // The rename will not complete until both a/b/c and a/b/empty become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Trigger errors on both inode loads
  builder_.triggerError(
      "a/b/c", std::domain_error("fake error for testing: src"));
  builder_.triggerError(
      "a/b/empty", std::domain_error("fake error for testing: dest"));

  // Verify the rename failure.
  // It doesn't matter which error we got, as long as one of
  // them was propated up.  (In practice our code currently propagates the
  // first error it receives.)
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      std::move(renameFuture).get(),
      std::domain_error,
      "fake error for testing: .*");
}
