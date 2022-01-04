/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <stdexcept>
#include <variant>
#include <vector>

#include "eden/fs/model/BlobMetadata.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/store/KeySpace.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/SerializedBlobMetadata.h"
#include "eden/fs/store/TreeMetadata.h"
#include "eden/fs/store/test/LocalStoreTest.h"

using namespace facebook::eden;

TEST_P(LocalStoreTest, testReadAndWriteTreeMetadata) {
  ObjectId hash{"3a8f8eb91101860fd8484154885838bf322964d0"};
  ObjectId childHash("8e073e366ed82de6465d1209d3f07da7eebabb93");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto size = childContents.size();

  auto childBlobMetadata = BlobMetadata{childSha1, size};
  TreeMetadata::HashIndexedEntryMetadata entryMetadata = {
      std::make_pair(childHash, childBlobMetadata)};
  auto treeMetadata = TreeMetadata{entryMetadata};
  auto serializedMetadata = treeMetadata.serialize();
  serializedMetadata.coalesce();

  store_->put(
      KeySpace::TreeMetaDataFamily,
      hash.getBytes(),
      folly::ByteRange(serializedMetadata.data(), serializedMetadata.length()));

  auto outResult = store_->get(KeySpace::TreeMetaDataFamily, hash);
  ASSERT_TRUE(outResult.isValid());

  auto outTreeMetadata = TreeMetadata::deserialize(outResult);
  auto outTreeEntryMetadata =
      std::get<TreeMetadata::HashIndexedEntryMetadata>(treeMetadata.entries());

  EXPECT_EQ(outTreeEntryMetadata.size(), outTreeEntryMetadata.size());

  auto outEntryMetadata = outTreeEntryMetadata.front();

  EXPECT_EQ(childHash, outEntryMetadata.first);
  EXPECT_EQ(childBlobMetadata.sha1, outEntryMetadata.second.sha1);
  EXPECT_EQ(childBlobMetadata.size, outEntryMetadata.second.size);
}

TEST_P(LocalStoreTest, testReadAndWriteTreeMetadataV2) {
  ObjectId hash{"3a8f8eb91101860fd8484154885838bf322964d0aabb"};
  ObjectId childHash("8e073e366ed82de6465d1209d3f07da7eebabb93ddee");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto size = childContents.size();

  auto childBlobMetadata = BlobMetadata{childSha1, size};
  TreeMetadata::HashIndexedEntryMetadata entryMetadata = {
      std::make_pair(childHash, childBlobMetadata)};
  auto treeMetadata = TreeMetadata{entryMetadata};
  auto serializedMetadata = treeMetadata.serialize();
  serializedMetadata.coalesce();

  store_->put(
      KeySpace::TreeMetaDataFamily,
      hash.getBytes(),
      folly::ByteRange(serializedMetadata.data(), serializedMetadata.length()));

  auto outResult = store_->get(KeySpace::TreeMetaDataFamily, hash);
  ASSERT_TRUE(outResult.isValid());

  auto outTreeMetadata = TreeMetadata::deserialize(outResult);
  auto outTreeEntryMetadata =
      std::get<TreeMetadata::HashIndexedEntryMetadata>(treeMetadata.entries());

  EXPECT_EQ(outTreeEntryMetadata.size(), outTreeEntryMetadata.size());

  auto outEntryMetadata = outTreeEntryMetadata.front();

  EXPECT_EQ(childHash, outEntryMetadata.first);
  EXPECT_EQ(childBlobMetadata.sha1, outEntryMetadata.second.sha1);
  EXPECT_EQ(childBlobMetadata.size, outEntryMetadata.second.size);
}

TEST_P(LocalStoreTest, testDeserializeEmptyMetadata) {
  StoreResult emptyResult{""};
  EXPECT_THROW(TreeMetadata::deserialize(emptyResult), std::invalid_argument);
}

TEST_P(LocalStoreTest, testDeserializeClippedTreeMetadata) {
  ObjectId childHash("8e073e366ed82de6465d1209d3f07da7eebabb93");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto size = childContents.size();

  auto childBlobMetadata = BlobMetadata{childSha1, size};
  TreeMetadata::HashIndexedEntryMetadata entryMetadata = {
      std::make_pair(childHash, childBlobMetadata)};
  auto treeMetadata = TreeMetadata{entryMetadata};
  auto serializedMetadata = treeMetadata.serialize();
  serializedMetadata.coalesce();
  auto serializedBytes = folly::ByteRange(
      serializedMetadata.data(),
      serializedMetadata.length() - Hash20::RAW_SIZE);

  EXPECT_THROW(
      TreeMetadata::deserialize(StoreResult(serializedBytes.toString())),
      std::invalid_argument);
}

TEST_P(LocalStoreTest, putTreeMetadata) {
  ObjectId hash{"3a8f8eb91101860fd8484154885838bf322964d0"};
  ObjectId childHash("8e073e366ed82de6465d1209d3f07da7eebabb93");

  StringPiece childContents("blah\n");
  auto childSha1 = Hash20::sha1(folly::ByteRange{childContents});
  auto size = childContents.size();

  auto childBlobMetadata = BlobMetadata{childSha1, size};
  TreeMetadata::HashIndexedEntryMetadata entryMetadata = {
      std::make_pair(childHash, childBlobMetadata)};
  auto treeMetadata = TreeMetadata{entryMetadata};

  std::vector<TreeEntry> entries;
  entries.emplace_back(
      childHash, PathComponent{childContents}, TreeEntryType::REGULAR_FILE);
  Tree tree{std::move(entries), hash};

  store_->putTreeMetadata(treeMetadata, tree);

  auto outChildResult = store_->getBlobMetadata(childHash).get();
  ASSERT_TRUE(outChildResult);
  EXPECT_EQ(childBlobMetadata.sha1, outChildResult.value().sha1);
  EXPECT_EQ(childBlobMetadata.size, outChildResult.value().size);

  auto outResult = store_->get(KeySpace::TreeMetaDataFamily, hash);

  ASSERT_TRUE(outResult.isValid());

  auto outTreeMetadata = TreeMetadata::deserialize(outResult);
  auto outTreeEntryMetadata =
      std::get<TreeMetadata::HashIndexedEntryMetadata>(treeMetadata.entries());

  EXPECT_EQ(outTreeEntryMetadata.size(), outTreeEntryMetadata.size());

  auto outEntryMetadata = outTreeEntryMetadata.front();
  EXPECT_EQ(childHash, outEntryMetadata.first);
  EXPECT_EQ(childBlobMetadata.sha1, outEntryMetadata.second.sha1);
  EXPECT_EQ(childBlobMetadata.size, outEntryMetadata.second.size);
}
