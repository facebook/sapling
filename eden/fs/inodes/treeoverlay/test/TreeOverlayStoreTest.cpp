/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/treeoverlay/TreeOverlayStore.h"

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

class TreeOverlayStoreTest : public ::testing::Test {
 protected:
  void SetUp() override {
    overlay_ = std::make_unique<TreeOverlayStore>(
        std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory));
    overlay_->createTableIfNonExisting();
    overlay_->loadCounters();
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
      entry.inodeNumber_ref() = overlay_->nextInodeNumber().get();
    }

    if (hash) {
      entry.hash_ref() = hash->toByteString();
    }

    return entry;
  }

  overlay::OverlayEntry makeEntry(InodeNumber inode) {
    return makeEntry(std::nullopt, dtype_t::Regular, inode);
  }

  std::unique_ptr<TreeOverlayStore> overlay_;
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

TEST_F(TreeOverlayStoreTest, testSaveLoadTree) {
  overlay::OverlayDir dir;

  dir.entries_ref()->emplace(std::make_pair(
      "hello",
      makeEntry(
          Hash20{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}, dtype_t::Dir)));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  overlay_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = overlay_->loadTree(kRootNodeId);
  ASSERT_EQ(dir.entries_ref()->size(), restored.entries_ref()->size());
  expect_entries(*dir.entries_ref(), *restored.entries_ref());
}

TEST_F(TreeOverlayStoreTest, testRecoverInodeEntryNumber) {
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("bar", makeEntry()));

  overlay_->saveTree(kRootNodeId, overlay::OverlayDir{dir});

  auto db = overlay_->takeDatabase();
  overlay_.reset();

  // Move sqlite handle from the previous overlay since the sqlite database is
  // created in-memory for testing.
  auto new_overlay = std::make_unique<TreeOverlayStore>(std::move(db));
  new_overlay->loadCounters();

  // Existing entry ID (4 items + 1 next) = 5
  EXPECT_EQ(new_overlay->nextEntryId_.load(), 5);
  // Existing inode ID (initial 2 + 4 items) = 6
  EXPECT_EQ(new_overlay->nextInode_.load(), 6);
}

TEST_F(TreeOverlayStoreTest, testSavingEmptyTree) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  overlay_->saveTree(inode, overlay::OverlayDir{dir});

  auto loaded = overlay_->loadTree(inode);
  EXPECT_EQ(loaded.entries_ref()->size(), 0);
}

TEST_F(TreeOverlayStoreTest, testSavingTreeOverwrite) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  overlay_->saveTree(inode, overlay::OverlayDir{dir});

  overlay::OverlayDir newDir;
  newDir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  overlay_->saveTree(inode, overlay::OverlayDir{newDir});

  auto loaded = overlay_->loadTree(inode);
  expect_entries(*newDir.entries_ref(), *loaded.entries_ref());
}

TEST_F(TreeOverlayStoreTest, testHasTree) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  EXPECT_FALSE(overlay_->hasTree(inode));

  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  overlay_->saveTree(inode, overlay::OverlayDir{dir});

  EXPECT_TRUE(overlay_->hasTree(inode));
  EXPECT_FALSE(overlay_->hasTree(InodeNumber{overlay_->nextInodeNumber()}));
}

TEST_F(TreeOverlayStoreTest, testRemoveTree) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));

  overlay_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 1);

  EXPECT_THROW(overlay_->removeTree(inode), TreeOverlayNonEmptyError);
  overlay_->removeChild(inode, "hello"_pc);
  overlay_->removeTree(inode);
  EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 0);
}

TEST_F(TreeOverlayStoreTest, testAddChild) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  overlay_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 0);

  overlay_->addChild(inode, "hello"_pc, makeEntry());
  auto loaded = overlay_->loadTree(inode);
  auto entries = loaded.entries_ref();
  EXPECT_EQ(entries->size(), 1);
  EXPECT_EQ(entries->begin()->first, "hello");

  overlay_->addChild(inode, "world"_pc, makeEntry());
  EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 2);
}

TEST_F(TreeOverlayStoreTest, testRemoveChild) {
  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries_ref()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  overlay_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 2);

  EXPECT_TRUE(overlay_->hasChild(inode, "hello"_pc));
  overlay_->removeChild(inode, "hello"_pc);
  auto loaded = overlay_->loadTree(inode);
  auto entries = loaded.entries_ref();
  EXPECT_EQ(entries->size(), 1);
  EXPECT_EQ(entries->begin()->first, "world");
  EXPECT_FALSE(overlay_->hasChild(inode, "hello"_pc));
}

TEST_F(TreeOverlayStoreTest, testRenameChild) {
  auto subdirInode = InodeNumber{overlay_->nextInodeNumber()};

  // Prepare a subdirectory with child inodes
  {
    overlay::OverlayDir dir;
    auto entry = makeEntry();
    dir.entries_ref()->emplace(std::make_pair("subdir_child", entry));
    overlay_->saveTree(subdirInode, overlay::OverlayDir{dir});
  }

  auto inode = InodeNumber{overlay_->nextInodeNumber()};
  overlay::OverlayDir dir;
  auto entry = makeEntry();
  auto subdir = makeEntry(subdirInode);
  dir.entries_ref()->emplace(std::make_pair("hello", entry));
  dir.entries_ref()->emplace(std::make_pair("world", makeEntry()));
  dir.entries_ref()->emplace(std::make_pair("subdir", subdir));
  overlay_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(
      overlay_->loadTree(inode).entries_ref()->size(), 3); // hello world subdir

  // mv hello newname
  overlay_->renameChild(inode, inode, "hello"_pc, "newname"_pc);
  {
    auto loaded = overlay_->loadTree(inode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 3); // newname world subdir

    auto it = entries->find("newname");
    EXPECT_EQ(it->first, "newname");
    expect_entry(it->second, entry);
  }

  // overwriting existing files
  // mv newname world
  overlay_->renameChild(inode, inode, "newname"_pc, "world"_pc);
  {
    auto loaded = overlay_->loadTree(inode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 2); // world subdir
    auto it = entries->find("world");
    EXPECT_EQ(it->first, "world");
    expect_entry(it->second, entry);
  }

  // mv newname subdir
  // this fails because subdir is non-empty
  EXPECT_THROW(
      overlay_->renameChild(inode, inode, "newname"_pc, "subdir"_pc),
      TreeOverlayNonEmptyError);

  overlay::OverlayDir anotherDir;
  auto anotherInode = InodeNumber{overlay_->nextInodeNumber()};
  overlay_->saveTree(anotherInode, overlay::OverlayDir{anotherDir});
  // No entires in the new directory yet
  EXPECT_EQ(overlay_->loadTree(anotherInode).entries_ref()->size(), 0);

  // mv world ../newdir/newplace
  overlay_->renameChild(inode, anotherInode, "world"_pc, "newplace"_pc);

  {
    // Old directory should only have subdir now.
    EXPECT_EQ(overlay_->loadTree(inode).entries_ref()->size(), 1);

    auto loaded = overlay_->loadTree(anotherInode);
    auto entries = loaded.entries_ref();
    EXPECT_EQ(entries->size(), 1);
    auto it = entries->begin();
    EXPECT_EQ(it->first, "newplace");
    expect_entry(it->second, entry);
  }
}
} // namespace facebook::eden
