/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/sqlitecatalog/SqliteTreeStore.h"

#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <memory>
#include <optional>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/utils/DirType.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

using namespace facebook::eden::path_literals;

class SqliteTreeStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    store_ = std::make_unique<SqliteTreeStore>(
        std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory));
    store_->createTableIfNonExisting();
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

  std::unique_ptr<SqliteTreeStore> store_;
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

TEST_F(SqliteTreeStoreTest, testSaveLoadTree) {
  overlay::OverlayDir dir;

  dir.entries_ref()->emplace(std::make_pair(
      "hello",
      makeEntry(
          Hash20{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}, dtype_t::Dir)));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = store_->loadTree(kRootNodeId);
  ASSERT_EQ(dir.entries_ref()->size(), restored.entries_ref()->size());
  expect_entries(*dir.entries_ref(), *restored.entries_ref());
}

TEST_F(SqliteTreeStoreTest, testRecoverInodeEntryNumber) {
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});

  auto db = store_->takeDatabase();
  store_.reset();

  // Move sqlite handle from the previous overlay since the sqlite database is
  // created in-memory for testing.
  auto new_store = std::make_unique<SqliteTreeStore>(std::move(db));
  new_store->loadCounters();

  // Existing entry ID (4 items + 1 next) = 5
  EXPECT_EQ(new_store->nextEntryId_.load(), 5);
  // Existing inode ID (initial 2 + 4 items) = 6
  EXPECT_EQ(new_store->nextInode_.load(), 6);
}

TEST_F(SqliteTreeStoreTest, testSavingEmptyTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  store_->saveTree(inode, overlay::OverlayDir{dir});

  auto loaded = store_->loadTree(inode);
  EXPECT_EQ(loaded.entries_ref()->size(), 0);
}

TEST_F(SqliteTreeStoreTest, testSavingTreeOverwrite) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});

  overlay::OverlayDir newDir;
  newDir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{newDir});

  auto loaded = store_->loadTree(inode);
  expect_entries(*newDir.entries_ref(), *loaded.entries_ref());
}

TEST_F(SqliteTreeStoreTest, testHasTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  EXPECT_FALSE(store_->hasTree(inode));

  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});

  EXPECT_TRUE(store_->hasTree(inode));
  EXPECT_FALSE(store_->hasTree(InodeNumber{store_->nextInodeNumber()}));
}

TEST_F(SqliteTreeStoreTest, testRemoveTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));

  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 1);

  EXPECT_THROW(store_->removeTree(inode), SqliteTreeStoreNonEmptyError);
  store_->removeChild(inode, "hello"_pc);
  store_->removeTree(inode);
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 0);
}

TEST_F(SqliteTreeStoreTest, testAddChild) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 0);

  store_->addChild(inode, "hello"_pc, makeEntry());
  auto loaded = store_->loadTree(inode);
  auto entries = loaded.entries_ref();
  EXPECT_EQ(entries->size(), 1);
  EXPECT_EQ(entries->begin()->first, "hello");

  store_->addChild(inode, "world"_pc, makeEntry());
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 2);
}

TEST_F(SqliteTreeStoreTest, testRemoveChild) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 2);

  EXPECT_TRUE(store_->hasChild(inode, "hello"_pc));
  store_->removeChild(inode, "hello"_pc);
  auto loaded = store_->loadTree(inode);
  auto entries = loaded.entries_ref();
  EXPECT_EQ(entries->size(), 1);
  EXPECT_EQ(entries->begin()->first, "world");
  EXPECT_FALSE(store_->hasChild(inode, "hello"_pc));
}

TEST_F(SqliteTreeStoreTest, testRenameChild) {
  auto subdirInode = InodeNumber{store_->nextInodeNumber()};

  // Prepare a subdirectory with child inodes
  {
    overlay::OverlayDir dir;
    auto entry = makeEntry();
    dir.entries_ref()->emplace(std::make_pair("subdir_child", entry));
    store_->saveTree(subdirInode, overlay::OverlayDir{dir});
  }

  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  auto entry = makeEntry();
  auto subdir = makeEntry(subdirInode);
  dir.entries_ref()->emplace(std::make_pair("hello", entry));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("subdir", subdir));
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(
      store_->loadTree(inode).entries_ref()->size(), 3); // hello world subdir

  // mv hello newname
  store_->renameChild(inode, inode, "hello"_pc, "newname"_pc);
  {
    auto loaded = store_->loadTree(inode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 3); // newname world subdir

    auto it = entries->find("newname");
    EXPECT_EQ(it->first, "newname");
    expect_entry(it->second, entry);
  }

  // overwriting existing files
  // mv newname world
  store_->renameChild(inode, inode, "newname"_pc, "world"_pc);
  {
    auto loaded = store_->loadTree(inode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 2); // world subdir
    auto it = entries->find("world");
    EXPECT_EQ(it->first, "world");
    expect_entry(it->second, entry);
  }

  // mv newname subdir
  // this fails because subdir is non-empty
  EXPECT_THROW(
      store_->renameChild(inode, inode, "newname"_pc, "subdir"_pc),
      SqliteTreeStoreNonEmptyError);

  overlay::OverlayDir anotherDir;
  auto anotherInode = InodeNumber{store_->nextInodeNumber()};
  store_->saveTree(anotherInode, overlay::OverlayDir{anotherDir});
  // No entires in the new directory yet
  EXPECT_EQ(store_->loadTree(anotherInode).entries_ref()->size(), 0);

  // mv world ../newdir/newplace
  store_->renameChild(inode, anotherInode, "world"_pc, "newplace"_pc);

  {
    // Old directory should only have subdir now.
    EXPECT_EQ(store_->loadTree(inode).entries_ref()->size(), 1);

    auto loaded = store_->loadTree(anotherInode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 1);
    auto it = entries->begin();
    EXPECT_EQ(it->first, "newplace");
    expect_entry(it->second, entry);
  }
}
} // namespace facebook::eden
