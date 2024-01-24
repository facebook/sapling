/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/lmdbcatalog/LMDBStoreInterface.h"

#include <folly/portability/GTest.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include <memory>
#include <optional>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/lmdb/LMDBDatabase.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/testharness/TempFile.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

using namespace facebook::eden::path_literals;

class LMDBStoreInterfaceTest : public ::testing::Test {
 protected:
  void SetUp() override {
    testDir_ = makeTempDir("eden_lmdb_tree_store_test");
    store_ = std::make_unique<LMDBStoreInterface>(
        std::make_unique<LMDBDatabase>(getLocalDir()));
    store_->loadCounters();
  }

  overlay::OverlayEntry makeEntry(
      std::optional<Hash20> hash = std::nullopt,
      dtype_t mode = dtype_t::Regular,
      std::optional<InodeNumber> inode = std::nullopt) {
    overlay::OverlayEntry entry;
    entry.mode_ref() = dtype_to_mode(mode);

    if (inode) {
      entry.inodeNumber_ref() = inode->get();
    } else {
      entry.inodeNumber_ref() = store_->nextInodeNumber().get();
    }

    if (hash) {
      entry.hash_ref() = hash->toByteString();
    }

    return entry;
  }

  overlay::OverlayEntry makeEntry(InodeNumber inode) {
    return makeEntry(std::nullopt, dtype_t::Regular, inode);
  }

  AbsolutePath getLocalDir() {
    return canonicalPath(testDir_.path().string());
  }

  folly::test::TemporaryDirectory testDir_;
  std::unique_ptr<LMDBStoreInterface> store_;
};

void expect_entry(
    const overlay::OverlayEntry& lhs,
    const overlay::OverlayEntry& rhs) {
  EXPECT_EQ(*lhs.inodeNumber_ref(), *rhs.inodeNumber_ref());
  EXPECT_EQ(*lhs.mode_ref(), *rhs.mode_ref());
  // We use `value_unchecked()` here since it will not throw an exception if
  // the value doesn't exist.
  EXPECT_EQ(lhs.hash_ref().value_unchecked(), rhs.hash_ref().value_unchecked());
}

void expect_entries(
    const std::map<std::string, overlay::OverlayEntry>& left,
    const std::map<std::string, overlay::OverlayEntry>& right) {
  auto lhs = left.begin();
  auto rhs = right.begin();
  for (; lhs != left.end() && rhs != right.end(); lhs++, rhs++) {
    EXPECT_EQ(lhs->first, rhs->first);
    expect_entry(lhs->second, rhs->second);
  }
}

TEST_F(LMDBStoreInterfaceTest, testSaveLoadTree) {
  overlay::OverlayDir dir;

  dir.entries_ref()->emplace(std::make_pair(
      "hello",
      makeEntry(
          Hash20{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}, dtype_t::Dir)));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(dir);
  store_->saveTree(kRootNodeId, std::move(serializedOverlayDir));
  auto restored = store_->loadTree(kRootNodeId);
  ASSERT_EQ(dir.entries_ref()->size(), restored.entries_ref()->size());
  expect_entries(*dir.entries_ref(), *restored.entries_ref());
}

TEST_F(LMDBStoreInterfaceTest, testRecoverInodeEntryNumber) {
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(kRootNodeId, std::move(serializedOverlayDir));

  auto db = store_->takeDatabase();
  store_.reset();

  // Move lmdb handle from the previous overlay since the lmdb database is
  // created in-memory for testing.
  auto new_store = std::make_unique<LMDBStoreInterface>(std::move(db));
  new_store->loadCounters();

  // Existing inode ID (initial 2 + 4 items) = 6
  EXPECT_EQ(new_store->nextInode_.load(), 6);
}

TEST_F(LMDBStoreInterfaceTest, testSavingEmptyTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(inode, std::move(serializedOverlayDir));

  auto loaded = store_->loadTree(inode);
  EXPECT_EQ(loaded.entries_ref()->size(), 0);
}

TEST_F(LMDBStoreInterfaceTest, testSavingEmptyBlob) {
  auto inode = InodeNumber{store_->nextInodeNumber()};

  char data[] = "";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);

  store_->saveBlob(inode, &iov, 1);

  auto loaded = store_->loadBlob(inode);

  std::string expectedData =
      std::string{static_cast<char*>(iov.iov_base), iov.iov_len};
  EXPECT_EQ(store_->loadBlob(inode), expectedData);
}

TEST_F(LMDBStoreInterfaceTest, testSavingTreeOverwrite) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(inode, std::move(serializedOverlayDir));

  overlay::OverlayDir newDir;
  newDir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  auto newSerializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(newDir);
  store_->saveTree(inode, std::move(newSerializedOverlayDir));

  auto loaded = store_->loadTree(inode);
  expect_entries(*newDir.entries_ref(), *loaded.entries_ref());
}

TEST_F(LMDBStoreInterfaceTest, testSavingBlobOverwrite) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  char data[] = "test contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);

  store_->saveBlob(inode, &iov, 1);

  char dataNew[] = "new data";
  struct iovec iovNew;
  iovNew.iov_base = dataNew;
  iovNew.iov_len = sizeof(dataNew);

  store_->saveBlob(inode, &iovNew, 1);

  std::string expectedData =
      std::string{static_cast<char*>(iovNew.iov_base), iovNew.iov_len};
  EXPECT_EQ(store_->loadBlob(inode), expectedData);
}

TEST_F(LMDBStoreInterfaceTest, testHasTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  EXPECT_FALSE(store_->hasTree(inode));

  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(inode, std::move(serializedOverlayDir));

  EXPECT_TRUE(store_->hasTree(inode));
  EXPECT_FALSE(store_->hasTree(InodeNumber{store_->nextInodeNumber()}));
}

TEST_F(LMDBStoreInterfaceTest, testHasBlob) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  EXPECT_FALSE(store_->hasBlob(inode));
  char data[] = "test contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);

  store_->saveBlob(inode, &iov, 1);

  EXPECT_TRUE(store_->hasBlob(inode));
  EXPECT_FALSE(store_->hasBlob(InodeNumber{store_->nextInodeNumber()}));
}

TEST_F(LMDBStoreInterfaceTest, testRemoveTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));

  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(inode, std::move(serializedOverlayDir));
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 1);

  EXPECT_NO_THROW(store_->removeTree(inode));
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 0);
}

TEST_F(LMDBStoreInterfaceTest, testRemoveBlob) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  char data[] = "test contents";
  struct iovec iov;
  iov.iov_base = data;
  iov.iov_len = sizeof(data);

  store_->saveBlob(inode, &iov, 1);

  EXPECT_NO_THROW(store_->removeBlob(inode));
}

TEST_F(LMDBStoreInterfaceTest, testLoadAndRemoveTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));

  auto serializedOverlayDir =
      apache::thrift::CompactSerializer::serialize<std::string>(std::move(dir));
  store_->saveTree(inode, std::move(serializedOverlayDir));
  EXPECT_EQ(store_->loadAndRemoveTree(inode).entries_ref()->size(), 1);
  EXPECT_FALSE(store_->hasTree(inode));

  EXPECT_EQ(store_->loadAndRemoveTree(inode).entries_ref()->size(), 0);
}

} // namespace facebook::eden
