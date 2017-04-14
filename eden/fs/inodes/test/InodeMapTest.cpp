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
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/test/TestChecks.h"

using namespace facebook::eden;
using folly::StringPiece;

TEST(InodeMap, invalidInodeNumber) {
  FakeTreeBuilder builder;
  builder.setFile("Makefile", "all:\necho success\n");
  builder.setFile("src/noop.c", "int main() { return 0; }\n");
  TestMount testMount{builder};

  EdenBugDisabler noCrash;
  auto* inodeMap = testMount.getEdenMount()->getInodeMap();
  auto future = inodeMap->lookupFileInode(0x12345678);
  EXPECT_THROW_RE(future.get(), std::runtime_error, "unknown inode number");
}

TEST(InodeMap, simpleLookups) {
  // Test simple lookups that succeed immediately from the LocalStore
  FakeTreeBuilder builder;
  builder.setFile("Makefile", "all:\necho success\n");
  builder.setFile("src/noop.c", "int main() { return 0; }\n");
  TestMount testMount{builder};
  auto* inodeMap = testMount.getEdenMount()->getInodeMap();

  // Look up the tree inode by name first
  auto root = testMount.getEdenMount()->getRootInode();
  auto srcTree = root->getOrLoadChild(PathComponentPiece{"src"}).get();

  // Next look up the tree by inode number
  auto tree2 = inodeMap->lookupTreeInode(srcTree->getNodeId()).get();
  EXPECT_EQ(srcTree, tree2);
  EXPECT_EQ(RelativePath{"src"}, tree2->getPath());

  // Next look up src/noop.c by name
  auto noop = tree2->getOrLoadChild(PathComponentPiece{"noop.c"}).get();
  EXPECT_NE(srcTree->getNodeId(), noop->getNodeId());

  // And look up src/noop.c by inode ID
  auto noop2 = inodeMap->lookupFileInode(noop->getNodeId()).get();
  EXPECT_EQ(noop, noop2);
  EXPECT_EQ(RelativePath{"src/noop.c"}, noop2->getPath());

  // lookupTreeInode() and lookupFileInode() should fail
  // when called on the wrong file type.
  EXPECT_THROW_ERRNO(
      inodeMap->lookupFileInode(srcTree->getNodeId()).get(), EISDIR);
  EXPECT_THROW_ERRNO(
      inodeMap->lookupTreeInode(noop->getNodeId()).get(), ENOTDIR);
}

TEST(InodeMap, asyncLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("README", "docs go here\n");
  builder.setFile("src/runme.sh", "#!/bin/sh\necho hello world\n", 0755);
  builder.setFile("src/test.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture = rootInode->getOrLoadChild(PathComponentPiece{"src"});
  EXPECT_FALSE(srcFuture.isReady());

  // Start a second lookup before the first is ready
  auto srcFuture2 = rootInode->getOrLoadChild(PathComponentPiece{"src"});
  EXPECT_FALSE(srcFuture2.isReady());

  // Now make the tree ready
  builder.setReady("src");
  ASSERT_TRUE(srcFuture.isReady());
  ASSERT_TRUE(srcFuture2.isReady());
  auto srcTree = srcFuture.get(std::chrono::seconds(1));
  auto srcTree2 = srcFuture2.get(std::chrono::seconds(1));
  EXPECT_EQ(srcTree.get(), srcTree2.get());
}

TEST(InodeMap, asyncError) {
  auto builder = FakeTreeBuilder();
  builder.setFile("README", "docs go here\n");
  builder.setFile("src/runme.sh", "#!/bin/sh\necho hello world\n", 0755);
  builder.setFile("src/test.txt", "this is a test file");
  TestMount testMount{builder, false};

  // Look up the "src" tree inode by name
  // The future should only be fulfilled when after we make the tree ready
  auto rootInode = testMount.getEdenMount()->getRootInode();
  auto srcFuture = rootInode->getOrLoadChild(PathComponentPiece{"src"});
  EXPECT_FALSE(srcFuture.isReady());

  // Start a second lookup before the first is ready
  auto srcFuture2 = rootInode->getOrLoadChild(PathComponentPiece{"src"});
  EXPECT_FALSE(srcFuture2.isReady());

  // Now fail the tree lookup
  builder.triggerError(
      "src", std::domain_error("rejecting lookup for src tree"));
  ASSERT_TRUE(srcFuture.isReady());
  ASSERT_TRUE(srcFuture2.isReady());
  EXPECT_THROW(srcFuture.get(), std::domain_error);
  EXPECT_THROW(srcFuture2.get(), std::domain_error);
}

TEST(InodeMap, recursiveLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInode() on the root
  auto rootFuture = edenMount->getInode(RelativePathPiece{""});
  ASSERT_TRUE(rootFuture.isReady());
  auto rootResult = rootFuture.get();
  EXPECT_EQ(edenMount->getRootInode(), rootResult);

  // Call EdenMount::getInode() to do a recursive lookup
  auto fileFuture = edenMount->getInode(RelativePathPiece{"a/b/c/d/file.txt"});
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d/file.txt");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d");
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = fileFuture.get();
  EXPECT_EQ(
      RelativePathPiece{"a/b/c/d/file.txt"}, fileInode->getPath().value());
}

TEST(InodeMap, recursiveLookupError) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInode() on the root
  auto rootFuture = edenMount->getInode(RelativePathPiece{""});
  ASSERT_TRUE(rootFuture.isReady());
  auto rootResult = rootFuture.get();
  EXPECT_EQ(edenMount->getRootInode(), rootResult);

  // Call EdenMount::getInode() to do a recursive lookup
  auto fileFuture = edenMount->getInode(RelativePathPiece{"a/b/c/d/file.txt"});
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b/c/d/file.txt");
  EXPECT_FALSE(fileFuture.isReady());
  builder.triggerError(
      "a/b/c/d", std::domain_error("error for testing purposes"));
  ASSERT_TRUE(fileFuture.isReady());
  EXPECT_THROW_RE(
      fileFuture.get(), std::domain_error, "error for testing purposes");
}

TEST(InodeMap, renameDuringRecursiveLookup) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInode() on the root
  auto rootFuture = edenMount->getInode(RelativePathPiece{""});
  ASSERT_TRUE(rootFuture.isReady());
  auto rootResult = rootFuture.get();
  EXPECT_EQ(edenMount->getRootInode(), rootResult);

  // Call EdenMount::getInode() to do a recursive lookup
  auto fileFuture = edenMount->getInode(RelativePathPiece{"a/b/c/d/file.txt"});
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  EXPECT_FALSE(fileFuture.isReady());

  auto bFuture = edenMount->getInode(RelativePathPiece{"a/b"});
  ASSERT_TRUE(bFuture.isReady());
  auto bInode = bFuture.get().asTreePtr();

  // Rename c to x after the recursive resolution should have
  // already looked it up
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  // Now mark the rest of the tree ready
  // Note that we don't actually have to mark the file itself ready.
  // The Inode lookup itself doesn't need the blob data yet.
  builder.setReady("a/b/c/d");
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = fileFuture.get();
  // We should have successfully looked up the inode, but it will report it
  // self (correctly) at its new path now.
  EXPECT_EQ(
      RelativePathPiece{"a/b/x/d/file.txt"}, fileInode->getPath().value());
}

TEST(InodeMap, renameDuringRecursiveLookupAndLoad) {
  auto builder = FakeTreeBuilder();
  builder.setFile("a/b/c/d/file.txt", "this is a test file");
  TestMount testMount{builder, false};
  const auto& edenMount = testMount.getEdenMount();

  // Call EdenMount::getInode() on the root
  auto rootFuture = edenMount->getInode(RelativePathPiece{""});
  ASSERT_TRUE(rootFuture.isReady());
  auto rootResult = rootFuture.get();
  EXPECT_EQ(edenMount->getRootInode(), rootResult);

  // Call EdenMount::getInode() to do a recursive lookup
  auto fileFuture = edenMount->getInode(RelativePathPiece{"a/b/c/d/file.txt"});
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a");
  EXPECT_FALSE(fileFuture.isReady());
  builder.setReady("a/b");
  EXPECT_FALSE(fileFuture.isReady());

  auto bFuture = edenMount->getInode(RelativePathPiece{"a/b"});
  ASSERT_TRUE(bFuture.isReady());
  auto bInode = bFuture.get().asTreePtr();

  // Rename c to x while the recursive resolution is still trying
  // to look it up.
  auto renameFuture =
      bInode->rename(PathComponentPiece{"c"}, bInode, PathComponentPiece{"x"});
  // The rename will not complete until C becomes ready
  EXPECT_FALSE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  builder.setReady("a/b/c");
  ASSERT_TRUE(renameFuture.isReady());
  EXPECT_FALSE(fileFuture.isReady());

  // Now mark the rest of the tree ready
  // Note that we don't actually have to mark the file itself ready.
  // The Inode lookup itself doesn't need the blob data yet.
  builder.setReady("a/b/c/d");
  ASSERT_TRUE(fileFuture.isReady());
  auto fileInode = fileFuture.get();
  // We should have successfully looked up the inode, but it will report it
  // self (correctly) at its new path now.
  EXPECT_EQ(
      RelativePathPiece{"a/b/x/d/file.txt"}, fileInode->getPath().value());
}
