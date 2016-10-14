/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <folly/io/IOBuf.h>
#include <gtest/gtest.h>
#include "eden/fs/model/Blob.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/store/testutil/FakeObjectStore.h"

using namespace facebook::eden;
using folly::IOBuf;
using std::unique_ptr;
using std::unordered_map;
using std::vector;

namespace {
Hash fileHash("0000000000000000000000000000000000000000");
Hash tree1Hash("1111111111111111111111111111111111111111");
Hash tree2Hash("2222222222222222222222222222222222222222");
Hash sha1Hash("3333333333333333333333333333333333333333");
Hash commHash("4444444444444444444444444444444444444444");
Hash blobHash("5555555555555555555555555555555555555555");
}

TEST(FakeObjectStore, getObjectsOfAllTypesFromStore) {
  FakeObjectStore store;

  // Test getTree().
  vector<TreeEntry> entries1;
  uint8_t rw_ = 0b110;
  entries1.emplace_back(fileHash, "a_file", FileType::REGULAR_FILE, rw_);
  Tree tree1(std::move(entries1), tree1Hash);
  store.addTree(std::move(tree1));
  auto foundTree = store.getTree(tree1Hash);
  EXPECT_TRUE(foundTree);
  EXPECT_EQ(tree1Hash, foundTree->getHash());

  // Test getBlob().
  unique_ptr<IOBuf> buf1(IOBuf::create(0));
  Blob blob1(blobHash, *buf1.get());
  store.addBlob(std::move(blob1));
  auto foundBlob = store.getBlob(blobHash);
  EXPECT_TRUE(foundBlob);
  EXPECT_EQ(blobHash, foundBlob->getHash());

  // Test getTreeForCommit().
  vector<TreeEntry> entries2;
  entries2.emplace_back(fileHash, "a_file", FileType::REGULAR_FILE, rw_);
  Tree tree2(std::move(entries2), tree2Hash);
  store.setTreeForCommit(commHash, std::move(tree2));
  auto foundTreeForCommit = store.getTreeForCommit(commHash);
  ASSERT_NE(nullptr, foundTreeForCommit.get());
  EXPECT_EQ(tree2Hash, foundTreeForCommit->getHash());

  // Test getSha1ForBlob().
  unique_ptr<IOBuf> buf2(IOBuf::create(0));
  Blob blob2(blobHash, *buf2.get());
  store.setSha1ForBlob(blob2, sha1Hash);
  auto foundSha1 = store.getSha1ForBlob(blob2.getHash());
  ASSERT_NE(nullptr, foundSha1.get());
  EXPECT_EQ(sha1Hash, *foundSha1.get());
}

TEST(FakeObjectStore, getMissingObjectReturnsNullptr) {
  FakeObjectStore store;
  Hash hash("4242424242424242424242424242424242424242");
  EXPECT_EQ(nullptr, store.getTree(hash).get());
  EXPECT_EQ(nullptr, store.getBlob(hash).get());
  EXPECT_EQ(nullptr, store.getTreeForCommit(hash).get());
  EXPECT_EQ(nullptr, store.getSha1ForBlob(hash).get());
}
