/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/ExceptionWrapper.h>
#include <folly/portability/GTest.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeInodeAccessLogger.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"

using namespace facebook::eden;
using namespace std::chrono_literals;

class InodeAccessLoggingTest : public ::testing::Test {
 protected:
  void SetUp() override {
    builder_.setFiles({
        {"src/a/b/1.txt", "This is src/a/b/1.txt.\n"},
        {"toplevel.txt", "toplevel\n"},
    });
    mount_.initialize(builder_);
  }

  void resetLogger() {
    auto logger = std::dynamic_pointer_cast<FakeInodeAccessLogger>(
        mount_.getInodeAccessLogger());
    logger->reset();
  }

  size_t getAccessCount() const {
    auto logger = std::dynamic_pointer_cast<FakeInodeAccessLogger>(
        mount_.getInodeAccessLogger());
    return logger->getAccessCount();
  }

  FakeTreeBuilder& getBuilder() {
    return builder_;
  }

  TestMount& getMount() {
    return mount_;
  }

  FakeTreeBuilder builder_;
  TestMount mount_;
};

#ifndef _WIN32
TEST_F(InodeAccessLoggingTest, statFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, statFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, writeFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->write("test", 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, writeFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->write("test", 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, setAttrFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  auto oldauxData = fileInode->getMetadata();
  DesiredMetadata sameMetadata{
      std::nullopt,
      oldauxData.mode,
      oldauxData.uid,
      oldauxData.gid,
      oldauxData.timestamps.atime.toTimespec(),
      oldauxData.timestamps.mtime.toTimespec()};

  fileInode->setattr(sameMetadata, ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, setAttrFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  auto oldauxData = fileInode->getMetadata();
  DesiredMetadata sameMetadata{
      std::nullopt,
      oldauxData.mode,
      oldauxData.uid,
      oldauxData.gid,
      oldauxData.timestamps.atime.toTimespec(),
      oldauxData.timestamps.mtime.toTimespec()};

  fileInode->setattr(sameMetadata, ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getxattrFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getxattr("user.sha1", ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
  resetLogger();

  fileInode->getxattr("user.blake3", ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getxattrFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getxattr("user.sha1", ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
  resetLogger();

  fileInode->getxattr("user.blake3", ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, ensureMaterializedFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, ensureMaterializedFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, ensureMaterializedSymlinkTopLevel) {
  auto rootInode = mount_.getRootInode();
  auto linkInode = rootInode->symlink(
      PathComponentPiece{"symlink.txt"},
      "toplevel.txt",
      InvalidationRequired::No);
  resetLogger();

  linkInode->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get(0ms);

  // no accesses logged because we're not following symlinks
  EXPECT_EQ(0, getAccessCount());

  linkInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  // 2 accesses logged, 1 for the symlink itself and 1 for the target
  EXPECT_EQ(2, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, ensureMaterializedSymlinkNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  auto linkInode = dirInode->symlink(
      PathComponentPiece{"symlink.txt"}, "1.txt", InvalidationRequired::No);
  resetLogger();

  linkInode->ensureMaterialized(ObjectFetchContext::getNullContext(), false)
      .get(0ms);

  // no accesses logged because we're not following symlinks
  EXPECT_EQ(0, getAccessCount());

  linkInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  // 2 accesses logged, 1 for the symlink itself and 1 for the target
  EXPECT_EQ(2, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, readLinkFileTopLevel) {
  auto rootInode = mount_.getRootInode();
  auto linkInode = rootInode->symlink(
      PathComponentPiece{"symlink.txt"},
      "toplevel.txt",
      InvalidationRequired::No);
  resetLogger();

  linkInode->readlink(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, readLinkFileNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  auto linkInode = dirInode->symlink(
      PathComponentPiece{"symlink.txt"}, "1.txt", InvalidationRequired::No);
  resetLogger();

  linkInode->readlink(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, readFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->read(10, 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, readFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->read(10, 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}
#endif

TEST_F(InodeAccessLoggingTest, readAllFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->readAll(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, readAllFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->readAll(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getSha1FileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getSha1(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getSha1FileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getSha1(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getBlake3FileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getBlake3(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getBlake3FileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getBlake3(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getBlobAuxDataFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getBlobAuxData(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, getBlobAuxDataFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getBlobAuxData(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

#ifdef __linux__
// Only run the fallocate tests on Linux because they are not supported on
// other platforms as per OverlayFile::fallocate(), but also because it is
// only registered in eden/fs/fuse/FuseChannel.cpp and not in
// eden/fs/nfs/Nfsd3.cpp
TEST_F(InodeAccessLoggingTest, fallocateFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->fallocate(0, 42, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, fallocateFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->fallocate(0, 42, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}
#endif

TEST_F(InodeAccessLoggingTest, statDirTopLevel) {
  auto dirInode = mount_.getTreeInode("src"_relpath);
  resetLogger();

  dirInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_F(InodeAccessLoggingTest, statDirNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  resetLogger();

  dirInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}
