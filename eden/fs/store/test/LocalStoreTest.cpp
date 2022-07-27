/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/store/test/LocalStoreTest.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/SqliteLocalStore.h"

namespace {

using namespace facebook::eden;
using namespace folly::string_piece_literals;
using namespace std::chrono_literals;

LocalStoreImplResult makeMemoryLocalStore(FaultInjector*) {
  return {std::nullopt, std::make_shared<MemoryLocalStore>()};
}

LocalStoreImplResult makeSqliteLocalStore(FaultInjector*) {
  auto tempDir = makeTempDir();
  auto store = std::make_shared<SqliteLocalStore>(
      AbsolutePathPiece{tempDir.path().string()} + "sqlite"_pc);
  return {std::move(tempDir), std::move(store)};
}

TEST_P(LocalStoreTest, testReadAndWriteBlob) {
  ObjectId hash = ObjectId::fromHex("3a8f8eb91101860fd8484154885838bf322964d0");

  StringPiece contents("{\n  \"breakConfig\": true\n}\n");
  auto buf =
      folly::IOBuf{folly::IOBuf::WRAP_BUFFER, folly::ByteRange{contents}};
  auto sha1 = Hash20::sha1(buf);

  auto inBlob = Blob{hash, std::move(buf)};
  store_->putBlob(hash, &inBlob);

  auto outBlob = store_->getBlob(hash).get(10s);
  EXPECT_EQ(hash.asHexString(), outBlob->getHash().asHexString());
  EXPECT_EQ(
      contents, outBlob->getContents().clone()->moveToFbString().toStdString());

  {
    auto retrievedMetadata = store_->getBlobMetadata(hash).get(10s);
    ASSERT_FALSE(retrievedMetadata.has_value());
  }

  store_->putBlobMetadata(hash, &inBlob);

  auto retrievedMetadata = store_->getBlobMetadata(hash).get(10s);
  ASSERT_TRUE(retrievedMetadata.has_value());

  EXPECT_EQ(sha1, retrievedMetadata.value().sha1);
  EXPECT_EQ(contents.size(), retrievedMetadata.value().size);
}

TEST_P(LocalStoreTest, testReadNonexistent) {
  using namespace std::chrono_literals;

  ObjectId hash = ObjectId::fromHex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
  EXPECT_TRUE(nullptr == store_->getBlob(hash).get(10s));
  auto retrievedMetadata = store_->getBlobMetadata(hash).get(10s);
  EXPECT_FALSE(retrievedMetadata.has_value());
}

TEST_P(LocalStoreTest, testReadsAndWriteTree) {
  using folly::unhexlify;
  using std::string;

  ObjectId hash = ObjectId::fromHex("8e073e366ed82de6465d1209d3f07da7eebabb93");

  auto gitTreeObject = folly::to<string>(
      string("tree 424\x00", 9),

      string("100644 .babelrc\x00", 16),
      unhexlify("3a8f8eb91101860fd8484154885838bf322964d0"),

      string("100644 .flowconfig\x00", 19),
      unhexlify("3610882f48696cc7ca0835929511c9db70acbec6"),

      string("100644 README.md\x00", 17),
      unhexlify("c5f15617ed29cd35964dc197a7960aeaedf2c2d5"),

      string("40000 lib\x00", 10),
      unhexlify("e95798e17f694c227b7a8441cc5c7dae50a187d0"),

      string("100755 nuclide-start-server\x00", 28),
      unhexlify("006babcf5734d028098961c6f4b6b6719656924b"),

      string("100644 package.json\x00", 20),
      unhexlify("582591e0f0d92cb63a85156e39abd43ebf103edc"),

      string("40000 scripts\x00", 14),
      unhexlify("e664fd28e60a0da25739fdf732f412ab3e91d1e1"),

      string("100644 services-3.json\x00", 23),
      unhexlify("3ead3c6cd723f4867bef4444ba18e6ffbf0f711a"),

      string("100644 services-config.json\x00", 28),
      unhexlify("bbc8e67499b7f3e1ea850eeda1253be7da5c9199"),

      string("40000 spec\x00", 11),
      unhexlify("3bae53a99d080dd851f78e36eb343320091a3d57"),

      string("100644 xdebug.ini\x00", 18),
      unhexlify("9ed5bbccd1b9b0077561d14c0130dc086ab27e04"));

  store_->put(
      KeySpace::TreeFamily, hash.getBytes(), StringPiece{gitTreeObject});
  auto tree = store_->getTree(hash).get(10s);
  EXPECT_EQ(
      "8e073e366ed82de6465d1209d3f07da7eebabb93",
      tree->getHash().asHexString());
  EXPECT_EQ(11, tree->size());

  auto readmeEntry = tree->find("README.md"_pc);
  EXPECT_EQ(
      "c5f15617ed29cd35964dc197a7960aeaedf2c2d5",
      readmeEntry->second.getHash().asHexString());
  EXPECT_EQ("README.md", readmeEntry->first);
  EXPECT_EQ(false, readmeEntry->second.isTree());
  EXPECT_EQ(TreeEntryType::REGULAR_FILE, readmeEntry->second.getType());
}

TEST_P(LocalStoreTest, testGetResult) {
  StringPiece key1 = "foo";
  StringPiece key2 = "bar";

  EXPECT_FALSE(store_->get(KeySpace::BlobFamily, key1).isValid());
  EXPECT_FALSE(store_->get(KeySpace::BlobFamily, key2).isValid());

  store_->put(KeySpace::BlobFamily, key1, "hello world"_sp);
  auto result1 = store_->get(KeySpace::BlobFamily, key1);
  ASSERT_TRUE(result1.isValid());
  EXPECT_EQ("hello world", result1.piece());

  auto result2 = store_->get(KeySpace::BlobFamily, key2);
  EXPECT_FALSE(result2.isValid());
  EXPECT_THROW(result2.piece(), std::domain_error);
}

TEST_P(LocalStoreTest, StoreResult_contains_keyspace_name_and_key) {
  auto key = ObjectId{kEmptySha1.getBytes()};
  auto result = store_->get(KeySpace::BlobFamily, key);
  try {
    result.asString();
    FAIL();
  } catch (std::domain_error& e) {
    EXPECT_EQ(
        std::string{
            "value not present in store: key da39a3ee5e6b4b0d3255bfef95601890afd80709 missing from blob keyspace"},
        e.what());
  }
}

TEST_P(LocalStoreTest, testMultipleBlobWriters) {
  StringPiece key1_1 = "foo";
  StringPiece key1_2 = "bar";

  StringPiece key1_3 = "john";
  StringPiece key1_4 = "doe";

  StringPiece key2_1 = "bender";
  StringPiece key2_2 = "bending";

  StringPiece key3_1 = "max";
  StringPiece key3_2 = "damage";

  auto batch1 = store_->beginWrite(8192);
  batch1->put(KeySpace::BlobFamily, key1_1, "hello world1_1"_sp);
  batch1->put(KeySpace::BlobFamily, key1_2, "hello world1_2"_sp);

  auto batch2 = store_->beginWrite(1024);
  batch2->put(KeySpace::BlobFamily, key2_1, "hello world2_1"_sp);
  batch2->put(KeySpace::BlobFamily, key2_2, "hello world2_2"_sp);

  auto batch3 = store_->beginWrite();
  batch3->put(KeySpace::BlobFamily, key3_1, "hello world3_1"_sp);
  batch3->put(KeySpace::BlobFamily, key3_2, "hello world3_2"_sp);

  batch1->put(KeySpace::BlobFamily, key1_3, "hello world1_3"_sp);
  batch1->put(KeySpace::BlobFamily, key1_4, "hello world1_4"_sp);

  batch1->flush();
  batch2->flush();

  auto result1_1 = store_->get(KeySpace::BlobFamily, key1_1);
  auto result2_1 = store_->get(KeySpace::BlobFamily, key2_1);
  auto result1_3 = store_->get(KeySpace::BlobFamily, key1_3);
  auto result1_4 = store_->get(KeySpace::BlobFamily, key1_4);

  EXPECT_FALSE(store_->get(KeySpace::BlobFamily, key3_1).isValid())
      << "key3_1 is not visible until flush";
  batch3->flush();
  auto result3_1 = store_->get(KeySpace::BlobFamily, key3_1);
  EXPECT_EQ("hello world3_1", result3_1.piece())
      << "key3_1 visible after flush";

  EXPECT_EQ("hello world1_1", result1_1.piece());
  EXPECT_EQ("hello world2_1", result2_1.piece());
  EXPECT_EQ("hello world1_4", result1_4.piece());
}

TEST_P(LocalStoreTest, testClearKeySpace) {
  store_->put(KeySpace::BlobFamily, "key1"_sp, "blob1"_sp);
  store_->put(KeySpace::BlobFamily, "key2"_sp, "blob2"_sp);
  store_->put(KeySpace::TreeFamily, "tree"_sp, "treeContents"_sp);
  store_->clearKeySpace(KeySpace::BlobFamily);
  EXPECT_FALSE(store_->hasKey(KeySpace::BlobFamily, "key1"_sp));
  EXPECT_FALSE(store_->hasKey(KeySpace::BlobFamily, "key2"_sp));
  EXPECT_TRUE(store_->hasKey(KeySpace::TreeFamily, "tree"_sp));
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
INSTANTIATE_TEST_CASE_P(
    Memory,
    LocalStoreTest,
    ::testing::Values(makeMemoryLocalStore));

INSTANTIATE_TEST_CASE_P(
    Sqlite,
    LocalStoreTest,
    ::testing::Values(makeSqliteLocalStore));
#pragma clang diagnostic pop

} // namespace

#endif
