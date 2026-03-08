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

class InodeAccessLoggingTest : public ::testing::TestWithParam<bool> {
 protected:
  void SetUp() override {
    builder_.setFiles({
        {"src/a/b/1.txt", "This is src/a/b/1.txt.\n"},
        {"toplevel.txt", "toplevel\n"},
    });
    mount_.initialize(builder_);

    if (GetParam()) {
      enableCoroutinesConfig(mount_);
    }
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
TEST_P(InodeAccessLoggingTest, statFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, statFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, writeFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->write("test", 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, writeFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->write("test", 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, setAttrFileTopLevel) {
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

TEST_P(InodeAccessLoggingTest, setAttrFileNested) {
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

TEST_P(InodeAccessLoggingTest, getxattrFileTopLevel) {
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

TEST_P(InodeAccessLoggingTest, getxattrFileNested) {
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

TEST_P(InodeAccessLoggingTest, ensureMaterializedFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, ensureMaterializedFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->ensureMaterialized(ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, ensureMaterializedSymlinkTopLevel) {
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

  // 2 accesses logged for reading the symlink and the target FileInodes
  EXPECT_EQ(2, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, ensureMaterializedSymlinkNested) {
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

  // 5 accesses logged, 2 for reading the symlink and the target FileInodes, and
  // 3 for symlink resolution (src, src/a, src/a/b)
  EXPECT_EQ(5, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, readLinkFileTopLevel) {
  auto rootInode = mount_.getRootInode();
  auto linkInode = rootInode->symlink(
      PathComponentPiece{"symlink.txt"},
      "toplevel.txt",
      InvalidationRequired::No);
  resetLogger();

  linkInode->readlink(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, readLinkFileNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  auto linkInode = dirInode->symlink(
      PathComponentPiece{"symlink.txt"}, "1.txt", InvalidationRequired::No);
  resetLogger();

  linkInode->readlink(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, readFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->read(10, 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, readFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->read(10, 0, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}
#endif

TEST_P(InodeAccessLoggingTest, readAllFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->readAll(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, readAllFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->readAll(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getSha1FileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getSha1(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getSha1FileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getSha1(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getBlake3FileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getBlake3(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getBlake3FileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->getBlake3(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getBlobAuxDataFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->getBlobAuxData(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getBlobAuxDataFileNested) {
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
TEST_P(InodeAccessLoggingTest, fallocateFileTopLevel) {
  auto fileInode = mount_.getFileInode("toplevel.txt"_relpath);
  resetLogger();

  fileInode->fallocate(0, 42, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, fallocateFileNested) {
  auto fileInode = mount_.getFileInode("src/a/b/1.txt"_relpath);
  resetLogger();

  fileInode->fallocate(0, 42, ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}
#endif

TEST_P(InodeAccessLoggingTest, statDirTopLevel) {
  auto dirInode = mount_.getTreeInode("src"_relpath);
  resetLogger();

  dirInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, statDirNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  resetLogger();

  dirInode->stat(ObjectFetchContext::getNullContext()).get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrFindChildDirTopLevel) {
  auto dirInode = mount_.getRootInode();
  resetLogger();

  dirInode->getOrFindChild("src"_pc, ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrFindChildDirNested) {
  auto dirInode = mount_.getTreeInode("src/a"_relpath);
  resetLogger();

  dirInode->getOrFindChild("b"_pc, ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrFindChildFileNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  resetLogger();

  dirInode
      ->getOrFindChild("1.txt"_pc, ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrFindChildFileTopLevel) {
  auto dirInode = mount_.getRootInode();
  resetLogger();

  dirInode
      ->getOrFindChild(
          "toplevel.txt"_pc, ObjectFetchContext::getNullContext(), true)
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildDirTopLevel) {
  auto dirInode = mount_.getRootInode();
  resetLogger();

  dirInode->getOrLoadChild("src"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildDirNested) {
  auto dirInode = mount_.getTreeInode("src/a"_relpath);
  resetLogger();

  dirInode->getOrLoadChild("b"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildFileNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  resetLogger();

  dirInode->getOrLoadChild("1.txt"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildFileTopLevel) {
  auto dirInode = mount_.getRootInode();
  resetLogger();

  dirInode
      ->getOrLoadChild("toplevel.txt"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildTreeTopLevel) {
  auto dirInode = mount_.getRootInode();
  resetLogger();

  dirInode->getOrLoadChildTree("src"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getOrLoadChildTreeNested) {
  auto dirInode = mount_.getTreeInode("src/a"_relpath);
  resetLogger();

  dirInode->getOrLoadChildTree("b"_pc, ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildRecursiveDirTopLevel) {
  auto rootInode = mount_.getRootInode();
  resetLogger();

  rootInode
      ->getChildRecursive("src"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildRecursiveFileTopLevel) {
  auto rootInode = mount_.getRootInode();
  resetLogger();

  rootInode
      ->getChildRecursive(
          "toplevel.txt"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildRecursiveDirNested) {
  auto rootInode = mount_.getRootInode();
  resetLogger();

  rootInode
      ->getChildRecursive(
          "src/a/b"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);

  // 2 accesses logged, for src looking for a and for src/a looking for b -  we
  // don't log the access to src because we don't log accesses to the root tree
  EXPECT_EQ(2, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildRecursiveFileNested) {
  auto rootInode = mount_.getRootInode();
  resetLogger();

  rootInode
      ->getChildRecursive(
          "src/a/b/1.txt"_relpath, ObjectFetchContext::getNullContext())
      .get(0ms);

  // 3 accesses logged, for src looking for a, for src/a looking for b, and for
  // src/a/b looking for 1.txt -  we don't log the access to src because we
  // don't log accesses to the root tree
  EXPECT_EQ(3, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, unlinkTopLevel) {
  auto dirInode = mount_.getRootInode();
  dirInode->mknod("made.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  dirInode
      ->unlink(
          "made.txt"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, unlinkNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  dirInode->mknod("made.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  dirInode
      ->unlink(
          "made.txt"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, rmdirTopLevel) {
  auto dirInode = mount_.getRootInode();
  dirInode->mkdir("made"_pc, 0, InvalidationRequired::No);
  resetLogger();

  dirInode
      ->rmdir(
          "made"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, rmdirNested) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  dirInode->mkdir("made"_pc, 0, InvalidationRequired::No);
  resetLogger();

  dirInode
      ->rmdir(
          "made"_pc,
          InvalidationRequired::No,
          ObjectFetchContext::getNullContext())
      .get(0ms);

  EXPECT_EQ(1, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildrenTopLevelLoad) {
  auto dirInode = mount_.getRootInode();
  dirInode->mkdir("childdir1"_pc, 0, InvalidationRequired::No);
  dirInode->mkdir("childdir2"_pc, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile1.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile2.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  auto futures =
      dirInode->getChildren(ObjectFetchContext::getNullContext(), true);

  std::for_each(futures.begin(), futures.end(), [](auto&& pair) {
    std::move(pair.second).get(0ms);
  });

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildrenNestedLoad) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  dirInode->mkdir("childdir1"_pc, 0, InvalidationRequired::No);
  dirInode->mkdir("childdir2"_pc, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile1.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile2.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  auto futures =
      dirInode->getChildren(ObjectFetchContext::getNullContext(), true);

  std::for_each(futures.begin(), futures.end(), [](auto&& pair) {
    std::move(pair.second).get(0ms);
  });

  // logs the 1 existing child (1.txt) and the 4 newly created children
  EXPECT_EQ(5, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildrenTopLevelNoLoad) {
  auto dirInode = mount_.getRootInode();
  dirInode->mkdir("childdir1"_pc, 0, InvalidationRequired::No);
  dirInode->mkdir("childdir2"_pc, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile1.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile2.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  auto futures =
      dirInode->getChildren(ObjectFetchContext::getNullContext(), false);

  std::for_each(futures.begin(), futures.end(), [](auto&& pair) {
    std::move(pair.second).get(0ms);
  });

  // No accesses logged because we don't log accesses to the root tree
  EXPECT_EQ(0, getAccessCount());
}

TEST_P(InodeAccessLoggingTest, getChildrenNestedNoLoad) {
  auto dirInode = mount_.getTreeInode("src/a/b"_relpath);
  dirInode->mkdir("childdir1"_pc, 0, InvalidationRequired::No);
  dirInode->mkdir("childdir2"_pc, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile1.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  dirInode->mknod(
      "childfile2.txt"_pc, S_IFREG | 0644, 0, InvalidationRequired::No);
  resetLogger();

  auto futures =
      dirInode->getChildren(ObjectFetchContext::getNullContext(), false);

  std::for_each(futures.begin(), futures.end(), [](auto&& pair) {
    std::move(pair.second).get(0ms);
  });

  EXPECT_EQ(5, getAccessCount());
}

INSTANTIATE_TEST_SUITE_P(
    InodeAccessLoggingTestVariants,
    InodeAccessLoggingTest,
    ::testing::Bool(),
    [](const ::testing::TestParamInfo<bool>& info) {
      return info.param ? "Coroutines" : "Futures";
    });
