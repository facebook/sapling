/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/inodes/InodeMap.h"

#include <folly/Bits.h>
#include <folly/Format.h>
#include <folly/String.h>
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/utils/Bug.h"
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;
using folly::StringPiece;

class RenameTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    TestMountBuilder builder;
    builder.addFiles({
        {"a/b/c/doc.txt", "This file is used for most of the file renames.\n"},
        {"a/readme.txt", "I exist to be replaced.\n"},
        {"a/b/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/d/readme.txt", "I exist to be replaced.\n"},
        {"a/b/c/d/e/f/readme.txt", "I exist to be replaced.\n"},
        {"a/x/y/z/readme.txt", "I exist to be replaced.\n"},
    });
    mount_ = builder.build();
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
  renameFuture.get();

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
  renameFuture.get();

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
  renameFuture.get();

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
  renameFuture.get();

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
  EXPECT_THROW_ERRNO(renameFuture.get(), expectedError);
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
  rmdirFuture.get();

  // Do the rename
  auto renameFuture = srcDir->rename(
      srcPath.basename(), destDir, PathComponentPiece{"test.txt"});

  // The rename should fail with ENOENT since the destination directory no
  // longer exists
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(renameFuture.get(), ENOENT);
}

/*
 * Rename tests where the source and destination inode objects
 * are not loaded yet when the rename starts.
 */
class RenameLoadingTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Set up a directory structure that we will use for most
    // of the tests below
    BaseTestMountBuilder builder;
    auto backingStore = builder.getBackingStore();

    // This sets up the following files:
    // - a/b/c/doc.txt
    // - a/b/c/readme.txt
    // - a/b/testdir/sample.txt
    // - a/b/empty/
    doc_ = backingStore->putBlob("documentation\n");
    readme_ = backingStore->putBlob("more docs\n");
    sample_ = backingStore->putBlob("Lorem ipsum dolor sit amet\n");
    a_b_c_ =
        backingStore->putTree({{"doc.txt", doc_}, {"readme.txt", readme_}});
    a_b_testdir_ = backingStore->putTree({{"sample.txt", sample_}});
    // Empty directories generally aren't tracked by source control,
    // but create one for testing purposes anyway.
    a_b_empty_ = backingStore->putTree({});
    a_b_ = backingStore->putTree(
        {{"c", a_b_c_}, {"testdir", a_b_testdir_}, {"empty", a_b_empty_}});
    a_ = backingStore->putTree({{"b", a_b_}});
    root_ = backingStore->putTree({{"a", a_}});
    builder.setCommit(makeTestHash("ccc"), root_->get().getHash());
    // build() will hang unless the root tree is ready.
    root_->setReady();
    mount_ = builder.build();
  }

  std::unique_ptr<TestMount> mount_;
  StoredBlob* doc_;
  StoredBlob* readme_;
  StoredBlob* sample_;
  StoredTree* a_b_c_;
  StoredTree* a_b_;
  StoredTree* a_b_testdir_;
  StoredTree* a_b_empty_;
  StoredTree* a_;
  StoredTree* root_;
};

TEST_F(RenameLoadingTest, renameDirSameDirectory) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename where the child inode ("a/b/c" in this case)
  // is not ready yet, because the data is not available from the BackingStore.
  //
  // For now we have to test this with a directory, and not a regular file,
  // since file inodes can always be loaded immediately (as long as their
  // parent inode is ready).  File inodes do not wait to load the blob data
  // from the backing store before creating the FileInode object.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  // The rename will not complete until a_b_c_ becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Now make a_b_c_ ready
  a_b_c_->setReady();
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  renameFuture.get();
}

TEST_F(RenameLoadingTest, renameWithLoadPending) {
  a_->setReady();
  a_b_->setReady();

  // Start a lookup on a/b/c before we start the rename
  auto inodeFuture =
      mount_->getEdenMount()->getInode(RelativePathPiece{"a/b/c"});
  EXPECT_FALSE(inodeFuture.isReady());

  // Perform a rename on a/b/c before that inode is ready.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  // The rename will not complete until a_b_c_ becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Now make a_b_c_ ready
  a_b_c_->setReady();

  // Both the load and the rename should have completed
  ASSERT_TRUE(inodeFuture.isReady());
  ASSERT_TRUE(renameFuture.isReady());

  // The rename should be successful
  EXPECT_FALSE(renameFuture.hasException());
  renameFuture.get();

  // From an API guarantee point of view, it would be fine for the load
  // to succeed or to fail with ENOENT here, since it was happening
  // concurrently with a rename() that moved the file away from the path we
  // requested.
  //
  // In practice our code currently always succeeds the load attempt.
  if (inodeFuture.hasException()) {
    EXPECT_THROW_ERRNO(inodeFuture.get(), ENOENT);
  } else {
    auto cInode = inodeFuture.get();
    EXPECT_EQ("a/b/x", cInode->getPath().value().stringPiece());
  }
}

TEST_F(RenameLoadingTest, loadWithRenamePending) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on a/b/c before that inode is ready.
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  // The rename will not complete until a_b_c_ becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Also start a lookup on a/b/c after starting the rename
  auto inodeFuture =
      mount_->getEdenMount()->getInode(RelativePathPiece{"a/b/c"});
  EXPECT_FALSE(inodeFuture.isReady());

  // Now make a_b_c_ ready
  a_b_c_->setReady();

  // Both the load and the rename should have completed
  ASSERT_TRUE(inodeFuture.isReady());
  ASSERT_TRUE(renameFuture.isReady());

  // The rename should be successful
  EXPECT_FALSE(renameFuture.hasException());
  renameFuture.get();

  // From an API guarantee point of view, it would be fine for the load
  // to succeed or to fail with ENOENT here, since it was happening
  // concurrently with a rename() that moved the file away from the path we
  // requested.
  //
  // In practice our code currently always succeeds the load attempt.
  if (inodeFuture.hasException()) {
    EXPECT_THROW_ERRNO(inodeFuture.get(), ENOENT);
  } else {
    auto cInode = inodeFuture.get();
    EXPECT_EQ("a/b/x", cInode->getPath().value().stringPiece());
  }
}

TEST_F(RenameLoadingTest, renameLoadFailure) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  // The rename will not complete until a_b_c_ becomes ready
  EXPECT_FALSE(renameFuture.isReady());

  // Fail the load of a_b_c_
  a_b_c_->triggerError(std::domain_error("fake error for testing"));
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      renameFuture.get(), std::domain_error, "fake error for testing");
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDest) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"empty"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a_b_c_ ready first
  a_b_c_->setReady();
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a_b_empty_ ready
  a_b_empty_->setReady();

  // Both the load and the rename should have completed
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  renameFuture.get();
}

TEST_F(RenameLoadingTest, renameLoadDestOtherOrder) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"empty"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a_b_empty_ ready first
  a_b_empty_->setReady();
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a_b_c_ ready
  a_b_c_->setReady();

  // Both the load and the rename should have completed
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(renameFuture.hasException());
  renameFuture.get();
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDestNonempty) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"testdir"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a_b_c_ ready first
  a_b_c_->setReady();
  EXPECT_FALSE(renameFuture.isReady());
  // Now make a_b_testdir_ ready
  a_b_testdir_->setReady();

  // The load should fail with ENOTEMPTY
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(renameFuture.get(), ENOTEMPTY);
}

// Test a rename that replaces a destination directory, where neither
// the source nor destination are ready yet.
TEST_F(RenameLoadingTest, renameLoadDestNonemptyOtherOrder) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"testdir"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a_b_testdir_ ready first.
  a_b_testdir_->setReady();
  // The rename could potentially fail now, but it is also be fine for it to
  // wait for the source directory to be ready too before it performs
  // validation.  Therefore go ahead and make the source directory ready too
  // without checking renameFuture.isReady()
  a_b_c_->setReady();

  // The load should fail with ENOTEMPTY
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_ERRNO(renameFuture.get(), ENOTEMPTY);
}

TEST_F(RenameLoadingTest, renameLoadDestFailure) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"empty"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Make a_b_c_ ready first
  a_b_c_->setReady();
  EXPECT_FALSE(renameFuture.isReady());
  // Now fail the load on a_b_empty_
  a_b_empty_->triggerError(std::domain_error("fake error for testing"));

  // Verify the rename failure
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      renameFuture.get(), std::domain_error, "fake error for testing");
}

TEST_F(RenameLoadingTest, renameLoadDestFailureOtherOrder) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"empty"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Fail the load on a_b_empty_ first
  a_b_empty_->triggerError(std::domain_error("fake error for testing"));
  // The rename may fail immediately, but it's also fine for it to wait
  // for the source load to finish too.  Therefore go ahead and finish the load
  // on a_b_c_ without checking renameFuture.isReady()
  a_b_c_->setReady();

  // Verify the rename failure
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      renameFuture.get(), std::domain_error, "fake error for testing");
}

TEST_F(RenameLoadingTest, renameLoadBothFailure) {
  a_->setReady();
  a_b_->setReady();

  // Perform a rename on "a/b/c" before it is ready
  auto bInode = mount_->getTreeInode("a/b");
  auto renameFuture = bInode->rename(
      PathComponentPiece{"c"}, bInode, PathComponentPiece{"empty"});
  // The rename will not complete until both a_b_c_ and a_b_empty_ become ready
  EXPECT_FALSE(renameFuture.isReady());

  // Trigger errors on both inode loads
  a_b_c_->triggerError(std::domain_error("fake error for testing: src"));
  a_b_empty_->triggerError(std::domain_error("fake error for testing: dest"));

  // Verify the rename failure.
  // It doesn't matter which error we got, as long as one of
  // them was propated up.  (In practice our code currently propagates the
  // first error it receives.)
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_THROW_RE(
      renameFuture.get(), std::domain_error, "fake error for testing: .*");
}
