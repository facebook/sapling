/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <stdexcept>
#include <variant>
#include <vector>

#include "eden/fs/model/Hash.h"
#include "eden/fs/model/Tree.h"
#include "eden/fs/model/TreeAuxData.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/test/LocalStoreTest.h"

using namespace facebook::eden;
using namespace folly;

TEST_P(LocalStoreTest, testReadAndWriteTree) {
  ObjectId id{"3a8f8eb91101860fd8484154885838bf322964d0aacc"};
  ObjectId childId1("8e073e366ed82de6465d1209d3f07da7eebabb93bbdd");
  ObjectId childId2("8e073e366ed82de6465d1209d3f07da7eebabb939988");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto childBlake3 = Hash32::blake3(folly::ByteRange{childContents});
  auto size = childContents.size();
  Tree::container entries{kPathMapDefaultCaseSensitive};
  entries.emplace(
      "entry1"_pc,
      childId1,
      TreeEntryType::REGULAR_FILE,
      size,
      childSha1,
      childBlake3);
  entries.emplace("entry2"_pc, childId2, TreeEntryType::REGULAR_FILE);

  StringPiece digest("blahblah");
  auto treeDigestHash = Hash32::blake3(folly::ByteRange{digest});
  auto treeDigestSize = 320;
  auto treeAuxPtr =
      std::make_shared<TreeAuxData>(std::move(treeDigestHash), treeDigestSize);

  auto tree = Tree{id, std::move(entries), treeAuxPtr};

  auto serialized = tree.serialize();
  serialized.coalesce();

  store_->put(
      KeySpace::TreeFamily,
      id.getBytes(),
      folly::ByteRange(serialized.data(), serialized.length()));

  auto outResult = store_->get(KeySpace::TreeFamily, id);
  ASSERT_TRUE(outResult.isValid());

  auto outTree = Tree::tryDeserialize(id, outResult.piece());

  ASSERT_TRUE(outTree);
  ASSERT_TRUE(outTree->getAuxData());

  EXPECT_EQ(*outTree, tree);
  EXPECT_EQ(outTree->getAuxData()->digestHash, treeAuxPtr->digestHash);
  EXPECT_EQ(outTree->getAuxData()->digestSize, treeAuxPtr->digestSize);
}

TEST_P(LocalStoreTest, testReadLegacyTree) {
  ObjectId id{"3a8f8eb91101860fd8484154885838bf322964d0aacc"};
  ObjectId childId1("8e073e366ed82de6465d1209d3f07da7eebabb93bbdd");
  ObjectId childId2("8e073e366ed82de6465d1209d3f07da7eebabb939988");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto childBlake3 = Hash32::blake3(folly::ByteRange{childContents});
  auto size = childContents.size();
  Tree::container entries{kPathMapDefaultCaseSensitive};
  entries.emplace(
      "entry1"_pc,
      childId1,
      TreeEntryType::REGULAR_FILE,
      size,
      childSha1,
      childBlake3);
  entries.emplace("entry2"_pc, childId2, TreeEntryType::REGULAR_FILE);

  auto tree = Tree{std::move(entries), id};

  auto serialized = tree.serialize_v1();
  serialized.coalesce();

  store_->put(
      KeySpace::TreeFamily,
      id.getBytes(),
      folly::ByteRange(serialized.data(), serialized.length()));

  auto outResult = store_->get(KeySpace::TreeFamily, id);
  ASSERT_TRUE(outResult.isValid());

  auto outTree = Tree::tryDeserialize(id, outResult.piece());

  ASSERT_TRUE(outTree);
  ASSERT_FALSE(outTree->getAuxData());

  EXPECT_EQ(*outTree, tree);
}
