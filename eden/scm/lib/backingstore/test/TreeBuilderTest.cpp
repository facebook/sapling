/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>
#include <algorithm>
#include <array>

#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/config/HgObjectIdFormat.h"
#include "eden/fs/model/ObjectId.h"
#include "eden/scm/lib/backingstore/include/ffi.h"

using namespace sapling;
using namespace facebook::eden;
using rust::Str;

class TreeBuilderTest : public ::testing::Test {
 protected:
  void SetUp() override {
    // Create a test object ID (20 bytes)
    std::array<uint8_t, 20> oid_bytes = {
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
        0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14};
    oid_ = ObjectId{folly::ByteRange{oid_bytes.data(), oid_bytes.size()}};

    // Create a test path
    path_ = RelativePathPiece{"test/path"};

    // Set up case sensitivity and object ID format
    caseSensitive_ = CaseSensitivity::Sensitive;
    objectIdFormat_ = HgObjectIdFormat::WithPath;
  }

  ObjectId oid_;
  RelativePathPiece path_;
  CaseSensitivity caseSensitive_;
  HgObjectIdFormat objectIdFormat_;
};

TEST_F(TreeBuilderTest, EmptyBuilder) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  EXPECT_EQ(builder.num_files(), 0);
  EXPECT_EQ(builder.num_dirs(), 0);

  auto tree = builder.build();
  EXPECT_EQ(tree->size(), 0);
  EXPECT_EQ(tree->getObjectId(), oid_);
}

TEST_F(TreeBuilderTest, AddFileEntry) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  // Create test data for a file entry
  Str fileName = "test_file.txt";
  std::array<uint8_t, 20> hgNode = {0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
                                    0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae,
                                    0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4};
  TreeEntryType entryType = TreeEntryType::REGULAR_FILE;
  uint64_t fileSize = 1024;
  std::array<uint8_t, 20> sha1Hash = {0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
                                      0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce,
                                      0xcf, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4};
  std::array<uint8_t, 32> blake3Hash = {
      0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb,
      0xec, 0xed, 0xee, 0xef, 0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6,
      0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff, 0x00};

  builder.add_entry_with_aux_data(
      fileName, hgNode, entryType, fileSize, sha1Hash, blake3Hash);

  EXPECT_EQ(builder.num_files(), 1);
  EXPECT_EQ(builder.num_dirs(), 0);

  auto tree = builder.build();
  EXPECT_EQ(tree->size(), 1);

  auto entry = tree->find(PathComponentPiece{"test_file.txt"})->second;

  EXPECT_EQ(entry.getType(), entryType);
  EXPECT_EQ(entry.getSize(), fileSize);
  EXPECT_EQ(
      entry.getContentSha1().value().getBytes(), folly::ByteRange{sha1Hash});
  EXPECT_EQ(
      entry.getContentBlake3().value().getBytes(),
      folly::ByteRange{blake3Hash});

  HgProxyHash parsedOid =
      facebook::eden::HgProxyHash::tryParseEmbeddedProxyHash(
          entry.getObjectId())
          .value();
  EXPECT_EQ(parsedOid.revHash().getBytes(), folly::ByteRange{hgNode});
  EXPECT_EQ(parsedOid.path(), path_ + PathComponentPiece{"test_file.txt"});
}

TEST_F(TreeBuilderTest, AddDirectoryEntry) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  // Create test data for a directory entry
  Str dirName = "test_dir";
  std::array<uint8_t, 20> hgNode = {0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
                                    0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae,
                                    0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4};
  TreeEntryType entryType = TreeEntryType::TREE;

  // Add the directory entry (we don't currently support dir aux data):
  builder.add_entry(dirName, hgNode, entryType);

  EXPECT_EQ(builder.num_files(), 0);
  EXPECT_EQ(builder.num_dirs(), 1);

  auto tree = builder.build();
  EXPECT_EQ(tree->size(), 1);

  auto entry = tree->find(PathComponentPiece{"test_dir"})->second;

  EXPECT_EQ(entry.getType(), entryType);

  HgProxyHash parsedOid =
      facebook::eden::HgProxyHash::tryParseEmbeddedProxyHash(
          entry.getObjectId())
          .value();
  EXPECT_EQ(parsedOid.revHash().getBytes(), folly::ByteRange{hgNode});
  EXPECT_EQ(parsedOid.path(), path_ + PathComponentPiece{"test_dir"});
}

TEST_F(TreeBuilderTest, AddMultipleEntries) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  // Add multiple file entries
  for (int i = 0; i < 3; ++i) {
    std::string fileName = "file" + std::to_string(i) + ".txt";
    Str fileNameStr(fileName);
    std::array<uint8_t, 20> hgNode = {0};
    hgNode[0] = static_cast<uint8_t>(i);

    builder.add_entry(fileNameStr, hgNode, TreeEntryType::REGULAR_FILE);
  }

  // Add multiple directory entries
  for (int i = 0; i < 2; ++i) {
    std::string dirName = "dir" + std::to_string(i);
    Str dirNameStr(dirName);
    std::array<uint8_t, 20> hgNode = {0};
    hgNode[1] = static_cast<uint8_t>(i);

    builder.add_entry(dirNameStr, hgNode, TreeEntryType::TREE);
  }

  EXPECT_EQ(builder.num_files(), 3);
  EXPECT_EQ(builder.num_dirs(), 2);

  auto tree = builder.build();
  EXPECT_EQ(tree->size(), 5);

  // Verify the files
  for (int i = 0; i < 3; ++i) {
    std::string fileName = "file" + std::to_string(i) + ".txt";
    auto entry = tree->find(PathComponentPiece{fileName})->second;

    EXPECT_EQ(entry.getType(), TreeEntryType::REGULAR_FILE);

    HgProxyHash parsedOid =
        facebook::eden::HgProxyHash::tryParseEmbeddedProxyHash(
            entry.getObjectId())
            .value();
    EXPECT_EQ(parsedOid.path(), path_ + PathComponentPiece{fileName});
  }

  // Verify the directories
  for (int i = 0; i < 2; ++i) {
    std::string dirName = "dir" + std::to_string(i);
    auto entry = tree->find(PathComponentPiece{dirName})->second;

    EXPECT_EQ(entry.getType(), TreeEntryType::TREE);

    HgProxyHash parsedOid =
        facebook::eden::HgProxyHash::tryParseEmbeddedProxyHash(
            entry.getObjectId())
            .value();
    EXPECT_EQ(parsedOid.path(), path_ + PathComponentPiece{dirName});
  }
}

TEST_F(TreeBuilderTest, SetAuxData) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  // Create aux data for the tree itself
  std::array<uint8_t, 32> digest = {
      0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
      0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16,
      0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20};
  uint64_t size = 2048;

  // Set aux data
  builder.set_aux_data(digest, size);

  auto tree = builder.build();

  // Verify the aux data
  auto auxData = tree->getAuxData();
  EXPECT_EQ(auxData->digestSize, size);
  EXPECT_EQ(auxData->digestHash->getBytes(), folly::ByteRange{digest});
}

TEST_F(TreeBuilderTest, MarkMissing) {
  TreeBuilder builder(oid_, path_, caseSensitive_, objectIdFormat_);

  builder.mark_missing();

  // Build should return nullptr when marked as missing
  auto tree = builder.build();
  EXPECT_EQ(tree, nullptr);
}
