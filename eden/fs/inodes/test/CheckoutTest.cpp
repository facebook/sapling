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
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using folly::StringPiece;

TEST(Checkout, simpleCheckout) {
  BaseTestMountBuilder builder;
  auto backingStore = builder.getBackingStore();

  // <root>
  // + src
  // | + test.c
  // | + main.c
  // + doc
  //   + readme.txt
  auto main1 = backingStore->putBlob("int main() { return 0; }\n");
  auto test1 = backingStore->putBlob("testy tests");
  auto src1 = backingStore->putTree({{"main.c", main1}, {"test.c", test1}});
  auto readme1 = backingStore->putBlob("all the words");
  auto doc1 = backingStore->putTree({{"readme.txt", readme1}});
  auto root1 = backingStore->putTree({{"src", src1}, {"doc", doc1}});

  builder.setCommit(makeTestHash("ccc"), root1->get().getHash());
  // build() will hang unless the root tree is ready.
  root1->setReady();

  auto testMount = builder.build();
  auto rootInode = testMount->getEdenMount()->getRootInode();

  // Call EdenMount::getInode() to do a recursive lookup
  // This is just to make sure some inodes are loaded when we do the checkout
  auto main1Future =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/main.c"});
  EXPECT_FALSE(main1Future.isReady());
  src1->setReady();
  ASSERT_TRUE(main1Future.isReady());
  auto main1Inode = main1Future.get().asFilePtr();

  auto test1Future =
      testMount->getEdenMount()->getInode(RelativePathPiece{"src/test.c"});
  ASSERT_TRUE(test1Future.isReady());
  auto test1Inode = test1Future.get().asFilePtr();
  EXPECT_TRUE(test1Inode->isSameAs(
      FakeBackingStore::makeBlob("testy tests"), S_IFREG | 0644));

  // Prepare a second tree
  auto test2 = backingStore->putBlob("even more testy tests");
  auto extra = backingStore->putBlob("extra stuff");
  auto src2 = backingStore->putTree({
      {"main.c", main1}, {"test.c", test2}, {"extra.h", extra},
  });
  auto root2 = backingStore->putTree({{"src", src2}, {"doc", doc1}});
  auto commit2 =
      backingStore->putCommit(makeTestHash("ddd"), root2->get().getHash());
  commit2->setReady();

  // Now do the checkout
  auto checkoutResult =
      testMount->getEdenMount()->checkout(makeTestHash("ddd"));
  EXPECT_FALSE(checkoutResult.isReady());
  root2->setReady();
  EXPECT_FALSE(checkoutResult.isReady());
  src2->setReady();
  EXPECT_FALSE(checkoutResult.isReady());
  test1->setReady();
  test2->setReady();
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
