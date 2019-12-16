/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/testharness/FakeObjectStore.h"
#include <folly/futures/Future.h>
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"

using namespace facebook::eden;
using folly::IOBuf;
using std::vector;

namespace {
Hash fileHash("0000000000000000000000000000000000000000");
Hash tree1Hash("1111111111111111111111111111111111111111");
Hash tree2Hash("2222222222222222222222222222222222222222");
Hash commHash("4444444444444444444444444444444444444444");
Hash blobHash("5555555555555555555555555555555555555555");
} // namespace

TEST(FakeObjectStore, getObjectsOfAllTypesFromStore) {
  FakeObjectStore store;

  // Test getTree().
  vector<TreeEntry> entries1;
  entries1.emplace_back(fileHash, "a_file", TreeEntryType::REGULAR_FILE);
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
  vector<TreeEntry> entries2;
  entries2.emplace_back(fileHash, "a_file", TreeEntryType::REGULAR_FILE);
  Tree tree2(std::move(entries2), tree2Hash);
  store.setTreeForCommit(commHash, std::move(tree2));
  auto foundTreeForCommit = store.getTreeForCommit(commHash).get();
  ASSERT_NE(nullptr, foundTreeForCommit.get());
  EXPECT_EQ(tree2Hash, foundTreeForCommit->getHash());
}

TEST(FakeObjectStore, getMissingObjectThrows) {
  FakeObjectStore store;
  Hash hash("4242424242424242424242424242424242424242");
  EXPECT_THROW(store.getTree(hash).get(), std::domain_error);
  EXPECT_THROW(store.getBlob(hash).get(), std::domain_error);
  EXPECT_THROW(store.getTreeForCommit(hash).get(), std::domain_error);
}
