/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/test/TestUtils.h>
#include <gtest/gtest.h>

#include "eden/fs/inodes/InodeMap.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/InodeUnloader.h"
#include "eden/fs/testharness/TestMount.h"

using namespace std::chrono_literals;
using folly::Optional;
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
    auto inode = edenMount->getInodeBlocking(relpath);
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
  // 4 files + 3 subdirectories + 1 root + 1 .eden + 3 .eden entries
  EXPECT_EQ(12, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());

  // Count includes files only, and the root's refcount will never go to zero
  // while the mount is up.
  EXPECT_EQ(11, this->unloader.unload(*edenMount->getRootInode()));

  EXPECT_EQ(1, inodeMap->getLoadedInodeCount());
  EXPECT_EQ(0, inodeMap->getUnloadedInodeCount());
}
