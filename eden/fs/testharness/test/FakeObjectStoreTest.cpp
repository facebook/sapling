/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeObjectStore.h"
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <folly/portability/GTest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/utils/ImmediateFuture.h"

using namespace facebook::eden;
using folly::IOBuf;
using std::vector;

namespace {
ObjectId fileHash("0000000000000000000000000000000000000000");
ObjectId tree1Hash("1111111111111111111111111111111111111111");
ObjectId tree2Hash("2222222222222222222222222222222222222222");
RootId commHash("4444444444444444444444444444444444444444");
ObjectId blobHash("5555555555555555555555555555555555555555");
} // namespace

TEST(FakeObjectStore, getObjectsOfAllTypesFromStore) {
  FakeObjectStore store;

  auto aFilePath = PathComponent{"a_file"};

  // Test getTree().
  Tree::container entries1{kPathMapDefaultCaseSensitive};
  entries1.emplace(aFilePath, fileHash, TreeEntryType::REGULAR_FILE);
  Tree tree1(std::move(entries1), tree1Hash);
  store.addTree(std::move(tree1));
  auto foundTree = store.getTree(tree1Hash).get();
  EXPECT_TRUE(foundTree);
  EXPECT_EQ(tree1Hash, foundTree->getHash());

  // Test getBlob().
  auto buf1 = IOBuf();
  Blob blob1(blobHash, buf1);
  store.addBlob(std::move(blob1));
  auto foundBlob = store.getBlob(blobHash).get();
  EXPECT_TRUE(foundBlob);
  EXPECT_EQ(blobHash, foundBlob->getHash());

  // Test getTreeForCommit().
  Tree::container entries2{kPathMapDefaultCaseSensitive};
  entries2.emplace(aFilePath, fileHash, TreeEntryType::REGULAR_FILE);
  Tree tree2(std::move(entries2), tree2Hash);
  store.setTreeForCommit(commHash, std::move(tree2));
  auto foundTreeForCommit = store.getRootTree(commHash).get();
  ASSERT_NE(nullptr, foundTreeForCommit.get());
  EXPECT_EQ(tree2Hash, foundTreeForCommit->getHash());
}

TEST(FakeObjectStore, getMissingObjectThrows) {
  FakeObjectStore store;
  ObjectId hash("4242424242424242424242424242424242424242");
  EXPECT_THROW(store.getTree(hash).get(), std::domain_error);
  EXPECT_THROW(store.getBlob(hash).get(), std::domain_error);
  RootId rootId{"missing"};
  EXPECT_THROW(store.getRootTree(rootId).get(), std::domain_error);
}
