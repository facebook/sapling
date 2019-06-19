/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/InodeUnloader.h"
#include "eden/fs/testharness/TestMount.h"

using namespace std::chrono_literals;
using namespace facebook::eden;

namespace {
template <typename Unloader>
struct UnloadTest : ::testing::Test {
  Unloader unloader;
};
} // namespace

TYPED_TEST_CASE(UnloadTest, InodeUnloaderTypes);

TYPED_TEST(UnloadTest, inodesAreUnloaded) {
  FakeTreeBuilder builder;
  builder.mkdir("docs");
  builder.setFile("docs/README.md", "readme");
  builder.setFile("docs/WholeFish", "sea bass");
  builder.mkdir("src");
  builder.setFile("src/code.c", "main() {}");
  builder.mkdir("test");
  builder.setFile("test/test.c", "TEST()");
  TestMount testMount{builder};

  const auto* edenMount = testMount.getEdenMount().get();
  auto inodeMap = edenMount->getInodeMap();

  std::vector<InodeNumber> loadedInodeNumbers;
  auto load = [&](RelativePathPiece relpath) -> InodeNumber {
    auto inode = edenMount->getInode(relpath).get();
    inode->incFuseRefcount();
    loadedInodeNumbers.push_back(inode->getNodeId());
    return inode->getNodeId();
  };

  // Load every file, increment the FUSE refcount, and remember its InodeNumber.
  auto readme_ino = load("docs/README.md"_relpath);
  auto wholefish_ino = load("docs/WholeFish"_relpath);
  auto code_ino = load("src/code.c"_relpath);
  auto test_ino = load("test/test.c"_relpath);

  EXPECT_TRUE(inodeMap->lookupInode(readme_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(wholefish_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(code_ino).get());
  EXPECT_TRUE(inodeMap->lookupInode(test_ino).get());

  // Now decrement the FUSE refcounts.
  inodeMap->decFuseRefcount(readme_ino, 1);
  inodeMap->decFuseRefcount(wholefish_ino, 1);
  inodeMap->decFuseRefcount(code_ino, 1);
  inodeMap->decFuseRefcount(test_ino, 1);

  // At this point, every file and tree should be loaded, plus the root and
  // .eden.
  // 4 files + 3 subdirectories + 1 root + 1 .eden + 4 .eden entries
  EXPECT_EQ(13, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());

  // Count includes files only, and the root's refcount will never go to zero
  // while the mount is up.
  EXPECT_EQ(12, this->unloader.unload(*edenMount->getRootInode()));

  EXPECT_EQ(1, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());
}

TYPED_TEST(UnloadTest, inodesCanBeUnloadedDuringLoad) {
  auto builder = FakeTreeBuilder{};
  builder.setFile("src/sub/file.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name, which starts the load.
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture = rootInode->getOrLoadChild("src"_pc);
  EXPECT_FALSE(srcFuture.isReady());

  rootInode->unloadChildrenNow();

  builder.setReady("src");
  ASSERT_TRUE(srcFuture.isReady());
  auto srcTree = std::move(srcFuture).get(1s).asTreePtr();
  EXPECT_NE(kRootNodeId, srcTree->getNodeId());

  auto subFuture = srcTree->getOrLoadChild("sub"_pc);
  srcTree.reset();
  EXPECT_FALSE(subFuture.isReady());

  rootInode->unloadChildrenNow();
  builder.setReady("src/sub");
  ASSERT_TRUE(subFuture.isReady());

  auto sub = std::move(subFuture).get(1s);
  EXPECT_NE(kRootNodeId, sub->getNodeId());
}

TEST(UnloadUnreferencedByFuse, inodesReferencedByFuseAreNotUnloaded) {
  FakeTreeBuilder builder;
  builder.mkdir("src");
  builder.setFile("src/file.txt", "contents");
  TestMount testMount{builder};

  const auto* edenMount = testMount.getEdenMount().get();
  auto inodeMap = edenMount->getInodeMap();

  auto inode = edenMount->getInode("src/file.txt"_relpath).get();
  inode->incFuseRefcount();
  inode.reset();

  // 1 file + 1 subdirectory + 1 root + 1 .eden + 4 .eden entries
  EXPECT_EQ(8, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());

  EXPECT_EQ(5, edenMount->getRootInode()->unloadChildrenUnreferencedByFuse());

  // root + src + file.txt
  EXPECT_EQ(3, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());
}
