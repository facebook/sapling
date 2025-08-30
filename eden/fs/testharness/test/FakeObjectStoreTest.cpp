/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeObjectStore.h"
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>

#include "eden/common/utils/ImmediateFuture.h"
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TestOps.h"
#include "eden/fs/model/Tree.h"

using namespace facebook::eden;
using folly::IOBuf;
using std::vector;

namespace {
ObjectId fileId("0000000000000000000000000000000000000000");
ObjectId tree1Id("1111111111111111111111111111111111111111");
ObjectId tree2Id("2222222222222222222222222222222222222222");
RootId commId("4444444444444444444444444444444444444444");
ObjectId blobId("5555555555555555555555555555555555555555");
} // namespace

TEST(FakeObjectStore, getObjectsOfAllTypesFromStore) {
  FakeObjectStore store;

  auto aFilePath = PathComponent{"a_file"};

  // Test getTree().
  Tree::container entries1{kPathMapDefaultCaseSensitive};
  entries1.emplace(aFilePath, fileId, TreeEntryType::REGULAR_FILE);
  Tree tree1(std::move(entries1), tree1Id);
  store.addTree(std::move(tree1));
  auto foundTree = store.getTree(tree1Id).get();
  EXPECT_TRUE(foundTree);
  EXPECT_EQ(tree1Id, foundTree->getObjectId());

  // Test getBlob().
  auto buf1 = IOBuf();
  Blob blob1(buf1);
  store.addBlob(blobId, std::move(blob1));
  auto foundBlob = store.getBlob(blobId).get();
  EXPECT_TRUE(foundBlob);

  // Test getTreeForCommit().
  Tree::container entries2{kPathMapDefaultCaseSensitive};
  entries2.emplace(aFilePath, fileId, TreeEntryType::REGULAR_FILE);
  Tree tree2(std::move(entries2), tree2Id);
  store.setTreeForCommit(commId, std::move(tree2));
  auto foundTreeForCommit = store.getRootTree(commId).get();
  ASSERT_NE(nullptr, foundTreeForCommit.tree.get());
  EXPECT_EQ(tree2Id, foundTreeForCommit.treeId);
}

TEST(FakeObjectStore, getMissingObjectThrows) {
  FakeObjectStore store;
  ObjectId id("4242424242424242424242424242424242424242");
  EXPECT_THROW(store.getTree(id).get(), std::domain_error);
  EXPECT_THROW(store.getBlob(id).get(), std::domain_error);
  RootId rootId{"missing"};
  EXPECT_THROW(store.getRootTree(rootId).get(), std::domain_error);
}
