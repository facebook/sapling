/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/fs/inodes/EdenMount.h"
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::StringPiece;

TEST(Checkout, simpleCheckout) {
  BaseTestMountBuilder builder;
  auto backingStore = builder.getBackingStore();

  auto srcBuilder = backingStore->treeBuilder();
  srcBuilder.setFile("src/main.c", "int main() { return 0; }\n");
  srcBuilder.setFile("src/test.c", "testy tests");
  srcBuilder.setFile("doc/readme.txt", "all the words");
  srcBuilder.finalize(false);

  auto testMount = builder.build(srcBuilder);

  // Call EdenMount::getInode() to do a recursive lookup
  // This is just to make sure some inodes are loaded when we do the checkout
  auto main1Future =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/main.c"});
  EXPECT_FALSE(main1Future.isReady());
  srcBuilder.setReady("src");
  ASSERT_TRUE(main1Future.isReady());
  auto main1Inode = main1Future.get().asFilePtr();

  auto test1Future =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/test.c"});
  ASSERT_TRUE(test1Future.isReady());
  auto test1Inode = test1Future.get().asFilePtr();
  EXPECT_TRUE(test1Inode->isSameAs(
      FakeBackingStore::makeBlob("testy tests"), S_IFREG | 0644));

  // Prepare a second tree
  auto destBuilder = srcBuilder.clone();
  destBuilder.replaceFile("src/test.c", "even more testy tests");
  destBuilder.setFile("src/extra.h", "extra stuff");
  destBuilder.finalize(false);

  auto commit2 = backingStore->putCommit("ddd", destBuilder);
  commit2->setReady();

  // Now do the checkout
  auto checkoutResult =
      testMount->getEdenMount()->checkout(makeTestHash("ddd"));
  EXPECT_FALSE(checkoutResult.isReady());
  destBuilder.setReady("");
  EXPECT_FALSE(checkoutResult.isReady());
  destBuilder.setReady("src");
  EXPECT_FALSE(checkoutResult.isReady());
  srcBuilder.setReady("src/test.c");
  EXPECT_FALSE(checkoutResult.isReady());
  destBuilder.setReady("src/test.c");
  ASSERT_TRUE(checkoutResult.isReady());
  auto results = checkoutResult.get();
  EXPECT_EQ(0, results.size());

  // Confirm that the tree has been updated correctly.
  auto test2Future =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/test.c"});
  ASSERT_TRUE(test2Future.isReady());
  auto test2Inode = test2Future.get().asFilePtr();
  EXPECT_FALSE(test2Inode->isSameAs(
      FakeBackingStore::makeBlob("testy tests"), S_IFREG | 0644));
  EXPECT_TRUE(test2Inode->isSameAs(
      FakeBackingStore::makeBlob("even more testy tests"), S_IFREG | 0644));

  auto extraFuture =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/extra.h"});
  ASSERT_TRUE(extraFuture.isReady());
  auto extraInode = extraFuture.get().asFilePtr();
  EXPECT_TRUE(extraInode->isSameAs(
      FakeBackingStore::makeBlob("extra stuff"), S_IFREG | 0644));
}
