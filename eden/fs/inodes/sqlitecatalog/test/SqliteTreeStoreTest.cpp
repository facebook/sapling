/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/inodes/sqlitecatalog/SqliteTreeStore.h"

#include <folly/Range.h>
#include <gtest/gtest.h>
#include <memory>
#include <optional>
#include "eden/common/utils/DirType.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/overlay/gen-cpp2/overlay_types.h"
#include "eden/fs/model/Hash.h"
#include "eden/fs/model/TreeEntry.h"
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/sqlite/SqliteStatement.h"

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
      std::optional<InodeNumber> inode = std::nullopt,
      bool isRestricted = false,
      std::optional<bool> hasACL = false) {
    overlay::OverlayEntry entry;
    entry.mode() = dtype_to_mode(mode);

    if (inode) {
      entry.inodeNumber() = inode->get();
    } else {
      entry.inodeNumber() = store_->nextInodeNumber().get();
    }

    if (hash) {
      entry.hash() = hash->toByteString();
    }

    entry.isRestricted() = isRestricted;
    entry.aclRootState() =
        static_cast<int32_t>(makeAclRootState(isRestricted, hasACL));

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
  EXPECT_EQ(*lhs.inodeNumber(), *rhs.inodeNumber());
  EXPECT_EQ(*lhs.mode(), *rhs.mode());
  // We use `value_unchecked()` here since it will not throw an exception if
  // the value doesn't exist.
  EXPECT_EQ(lhs.hash().value_unchecked(), rhs.hash().value_unchecked());
  EXPECT_EQ(*lhs.isRestricted(), *rhs.isRestricted());
  EXPECT_EQ(lhs.aclRootState().has_value(), rhs.aclRootState().has_value());
  if (lhs.aclRootState().has_value() && rhs.aclRootState().has_value()) {
    EXPECT_EQ(*lhs.aclRootState(), *rhs.aclRootState());
  }
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

AclRootState getAclRootState(const overlay::OverlayEntry& entry) {
  return static_cast<AclRootState>(*entry.aclRootState());
}

std::optional<bool> getHasACL(const overlay::OverlayEntry& entry) {
  return hasACLFromAclRootState(getAclRootState(entry));
}

TEST_F(SqliteTreeStoreTest, testSaveLoadTree) {
  overlay::OverlayDir dir;

  dir.entries()->emplace(
      std::make_pair(
          "hello",
          makeEntry(
              Hash20{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
              dtype_t::Dir)));
  dir.entries()->emplace(std::make_pair("world", makeEntry()));
  dir.entries()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries()->emplace(std::make_pair("bar", makeEntry()));

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = store_->loadTree(kRootNodeId);
  ASSERT_EQ(dir.entries()->size(), restored.entries()->size());
  expect_entries(*dir.entries(), *restored.entries());
}

TEST_F(SqliteTreeStoreTest, testSaveLoadTreePreservesAclMetadata) {
  overlay::OverlayDir dir;
  auto addEntry = [&](const char* name,
                      const char* hash,
                      bool isRestricted,
                      std::optional<bool> hasACL) {
    dir.entries()->emplace(
        std::make_pair(
            name,
            makeEntry(
                Hash20{hash},
                dtype_t::Dir,
                std::nullopt,
                isRestricted,
                hasACL)));
  };

  addEntry(
      "restricted", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", true, true);
  addEntry(
      "visible_under_acl",
      "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      false,
      true);
  addEntry(
      "known_without_acl",
      "cccccccccccccccccccccccccccccccccccccccc",
      false,
      false);

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = store_->loadTree(kRootNodeId);

  auto expectEntry = [&](const char* name,
                         bool isRestricted,
                         AclRootState aclRootState,
                         std::optional<bool> hasACL) {
    const auto& entry = restored.entries()->at(name);
    EXPECT_EQ(*entry.isRestricted(), isRestricted);
    EXPECT_EQ(getAclRootState(entry), aclRootState);
    EXPECT_EQ(getHasACL(entry), hasACL);
  };

  expectEntry("restricted", true, AclRootState::RestrictedAclRoot, true);
  expectEntry("visible_under_acl", false, AclRootState::AclRoot, true);
  expectEntry("known_without_acl", false, AclRootState::NoAcl, false);
}

TEST_F(SqliteTreeStoreTest, testSaveLoadTreePreservesUnknownAclMetadata) {
  overlay::OverlayDir dir;
  dir.entries()->emplace(
      std::make_pair(
          "unknown_acl",
          makeEntry(
              Hash20{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
              dtype_t::Dir,
              std::nullopt,
              false,
              std::nullopt)));

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = store_->loadTree(kRootNodeId);

  const auto& unknownAcl = restored.entries()->at("unknown_acl");
  EXPECT_EQ(getAclRootState(unknownAcl), AclRootState::Unknown);
  EXPECT_FALSE(getHasACL(unknownAcl).has_value());
}

TEST_F(SqliteTreeStoreTest, testRecoverInodeEntryNumber) {
  overlay::OverlayDir dir;
  dir.entries()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries()->emplace(std::make_pair("world", makeEntry()));
  dir.entries()->emplace(std::make_pair("foo", makeEntry()));
  dir.entries()->emplace(std::make_pair("bar", makeEntry()));

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
  EXPECT_EQ(loaded.entries()->size(), 0);
}

TEST_F(SqliteTreeStoreTest, testSavingTreeOverwrite) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries()->emplace(std::make_pair("hello", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});

  overlay::OverlayDir newDir;
  newDir.entries()->emplace(std::make_pair("world", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{newDir});

  auto loaded = store_->loadTree(inode);
  expect_entries(*newDir.entries(), *loaded.entries());
}

TEST_F(SqliteTreeStoreTest, testHasTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  EXPECT_FALSE(store_->hasTree(inode));

  overlay::OverlayDir dir;
  dir.entries()->emplace(std::make_pair("hello", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});

  EXPECT_TRUE(store_->hasTree(inode));
  EXPECT_FALSE(store_->hasTree(InodeNumber{store_->nextInodeNumber()}));
}

TEST_F(SqliteTreeStoreTest, testRemoveTree) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries()->emplace(std::make_pair("hello", makeEntry()));

  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 1);

  EXPECT_THROW(store_->removeTree(inode), SqliteTreeStoreNonEmptyError);
  store_->removeChild(inode, "hello"_pc);
  store_->removeTree(inode);
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 0);
}

TEST_F(SqliteTreeStoreTest, testAddChild) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 0);

  store_->addChild(inode, "hello"_pc, makeEntry());
  auto loaded = store_->loadTree(inode);
  auto entries = loaded.entries();
  EXPECT_EQ(entries->size(), 1);
  EXPECT_EQ(entries->begin()->first, "hello");

  store_->addChild(inode, "world"_pc, makeEntry());
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 2);
}

TEST_F(SqliteTreeStoreTest, testRemoveChild) {
  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  dir.entries()->emplace(std::make_pair("hello", makeEntry()));
  dir.entries()->emplace(std::make_pair("world", makeEntry()));
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 2);

  EXPECT_TRUE(store_->hasChild(inode, "hello"_pc));
  store_->removeChild(inode, "hello"_pc);
  auto loaded = store_->loadTree(inode);
  auto entries = loaded.entries();
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
    dir.entries()->emplace(std::make_pair("subdir_child", entry));
    store_->saveTree(subdirInode, overlay::OverlayDir{dir});
  }

  auto inode = InodeNumber{store_->nextInodeNumber()};
  overlay::OverlayDir dir;
  auto entry = makeEntry();
  auto subdir = makeEntry(subdirInode);
  dir.entries()->emplace(std::make_pair("hello", entry));
  dir.entries()->emplace(std::make_pair("world", makeEntry()));
  dir.entries()->emplace(std::make_pair("subdir", subdir));
  store_->saveTree(inode, overlay::OverlayDir{dir});
  EXPECT_EQ(store_->loadTree(inode).entries()->size(), 3); // hello world subdir

  // mv hello newname
  store_->renameChild(inode, inode, "hello"_pc, "newname"_pc);
  {
    auto loaded = store_->loadTree(inode);
    auto entries = loaded.entries();
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
    auto entries = loaded.entries();
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
  // No entries in the new directory yet
  EXPECT_EQ(store_->loadTree(anotherInode).entries()->size(), 0);

  // mv world ../newdir/newplace
  store_->renameChild(inode, anotherInode, "world"_pc, "newplace"_pc);

  {
    // Old directory should only have subdir now.
    EXPECT_EQ(store_->loadTree(inode).entries()->size(), 1);

    auto loaded = store_->loadTree(anotherInode);
    auto entries = loaded.entries();
    EXPECT_EQ(entries->size(), 1);
    auto it = entries->begin();
    EXPECT_EQ(it->first, "newplace");
    expect_entry(it->second, entry);
  }
}

TEST_F(SqliteTreeStoreTest, testIsRestrictedRoundTrip) {
  overlay::OverlayDir dir;
  dir.entries()->emplace(
      std::make_pair(
          "restricted_dir",
          makeEntry(
              Hash20{"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
              dtype_t::Dir,
              std::nullopt,
              true)));
  dir.entries()->emplace(
      std::make_pair(
          "normal_dir",
          makeEntry(
              Hash20{"cccccccccccccccccccccccccccccccccccccccc"},
              dtype_t::Dir)));

  store_->saveTree(kRootNodeId, overlay::OverlayDir{dir});
  auto restored = store_->loadTree(kRootNodeId);
  ASSERT_EQ(dir.entries()->size(), restored.entries()->size());
  expect_entries(*dir.entries(), *restored.entries());

  // Explicitly verify the restricted flags
  EXPECT_TRUE(*restored.entries()->at("restricted_dir").isRestricted());
  EXPECT_FALSE(*restored.entries()->at("normal_dir").isRestricted());
}

TEST_F(SqliteTreeStoreTest, testMigrationFromV1) {
  // Create a fresh in-memory database with the v1 schema (no is_restricted)
  auto db = std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory);
  db->transaction([&](auto& txn) {
    SqliteStatement(
        txn,
        "CREATE TABLE IF NOT EXISTS entries"
        "("
        "  parent INTEGER NOT NULL,"
        "  name STRING NOT NULL,"
        "  dtype INTEGER NOT NULL,"
        "  inode INTEGER NOT NULL,"
        "  sequence_id INTEGER NOT NULL,"
        "  hash BLOB,"
        "  PRIMARY KEY (parent, name)"
        ") WITHOUT ROWID")
        .step();
    SqliteStatement(txn, "PRAGMA user_version = 1").step();
    // Insert a v1 row (6 columns, no is_restricted)
    auto insertStmt = SqliteStatement(
        txn,
        "INSERT INTO entries (parent, name, dtype, inode, sequence_id, hash)"
        " VALUES (?, ?, ?, ?, ?, ?)");
    insertStmt.bind(1, kRootNodeId.get());
    insertStmt.bind(2, folly::StringPiece{"old_entry"});
    insertStmt.bind(3, static_cast<uint32_t>(dtype_t::Dir));
    insertStmt.bind(4, static_cast<uint64_t>(2));
    insertStmt.bind(5, static_cast<uint64_t>(0));
    insertStmt.bind(6, folly::ByteRange{});
    insertStmt.step();
  });

  // Construct SqliteTreeStore from the v1 database — triggers migration
  auto migrated_store = std::make_unique<SqliteTreeStore>(std::move(db));
  migrated_store->createTableIfNonExisting();
  migrated_store->loadCounters();

  // Verify the migrated row loads with isRestricted = false
  auto loaded = migrated_store->loadTree(kRootNodeId);
  ASSERT_EQ(loaded.entries()->size(), 1);
  const auto& oldEntry = loaded.entries()->at("old_entry");
  EXPECT_FALSE(*oldEntry.isRestricted());
  EXPECT_EQ(getAclRootState(oldEntry), AclRootState::Unknown);
  EXPECT_FALSE(getHasACL(oldEntry).has_value());

  // Verify new restricted entries work on the migrated schema
  migrated_store->addChild(
      kRootNodeId,
      "new_restricted"_pc,
      makeEntry(std::nullopt, dtype_t::Dir, InodeNumber{3}, true));

  auto reloaded = migrated_store->loadTree(kRootNodeId);
  ASSERT_EQ(reloaded.entries()->size(), 2);
  EXPECT_TRUE(*reloaded.entries()->at("new_restricted").isRestricted());
  EXPECT_FALSE(*reloaded.entries()->at("old_entry").isRestricted());
}

TEST_F(SqliteTreeStoreTest, testMigrationFromPartiallyAppliedV1) {
  auto db = std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory);
  db->transaction([&](auto& txn) {
    SqliteStatement(
        txn,
        "CREATE TABLE IF NOT EXISTS entries"
        "("
        "  parent INTEGER NOT NULL,"
        "  name STRING NOT NULL,"
        "  dtype INTEGER NOT NULL,"
        "  inode INTEGER NOT NULL,"
        "  sequence_id INTEGER NOT NULL,"
        "  hash BLOB,"
        "  is_restricted INTEGER NOT NULL DEFAULT 0,"
        "  PRIMARY KEY (parent, name)"
        ") WITHOUT ROWID")
        .step();
    SqliteStatement(txn, "PRAGMA user_version = 1").step();

    auto insertStmt = SqliteStatement(
        txn,
        "INSERT INTO entries "
        "(parent, name, dtype, inode, sequence_id, hash, is_restricted)"
        " VALUES (?, ?, ?, ?, ?, ?, ?)");
    insertStmt.bind(1, kRootNodeId.get());
    insertStmt.bind(2, folly::StringPiece{"restricted_entry"});
    insertStmt.bind(3, static_cast<uint32_t>(dtype_t::Dir));
    insertStmt.bind(4, static_cast<uint64_t>(2));
    insertStmt.bind(5, static_cast<uint64_t>(0));
    insertStmt.bind(6, folly::ByteRange{});
    insertStmt.bind(7, static_cast<uint64_t>(1));
    insertStmt.step();
  });

  auto migratedStore = std::make_unique<SqliteTreeStore>(std::move(db));
  migratedStore->createTableIfNonExisting();
  migratedStore->loadCounters();

  auto loaded = migratedStore->loadTree(kRootNodeId);
  ASSERT_EQ(loaded.entries()->size(), 1);
  const auto& entry = loaded.entries()->at("restricted_entry");
  EXPECT_TRUE(*entry.isRestricted());
  EXPECT_EQ(getAclRootState(entry), AclRootState::RestrictedAclRoot);
}

TEST_F(SqliteTreeStoreTest, testInvalidAclRootStateFallsBackToLegacyFlag) {
  auto db = std::make_unique<SqliteDatabase>(SqliteDatabase::inMemory);
  db->transaction([&](auto& txn) {
    SqliteStatement(
        txn,
        "CREATE TABLE IF NOT EXISTS entries"
        "("
        "  parent INTEGER NOT NULL,"
        "  name STRING NOT NULL,"
        "  dtype INTEGER NOT NULL,"
        "  inode INTEGER NOT NULL,"
        "  sequence_id INTEGER NOT NULL,"
        "  hash BLOB,"
        "  is_restricted INTEGER NOT NULL DEFAULT 0,"
        "  acl_root_state INTEGER,"
        "  PRIMARY KEY (parent, name)"
        ") WITHOUT ROWID")
        .step();
    SqliteStatement(txn, "PRAGMA user_version = 3").step();

    auto insertEntry =
        [&](folly::StringPiece name, uint64_t inode, bool isRestricted) {
          auto insertStmt = SqliteStatement(
              txn,
              "INSERT INTO entries "
              "(parent, name, dtype, inode, sequence_id, hash, is_restricted, "
              "acl_root_state)"
              " VALUES (?, ?, ?, ?, ?, ?, ?, ?)");
          insertStmt.bind(1, kRootNodeId.get());
          insertStmt.bind(2, name);
          insertStmt.bind(3, static_cast<uint32_t>(dtype_t::Dir));
          insertStmt.bind(4, inode);
          insertStmt.bind(5, static_cast<uint64_t>(0));
          insertStmt.bind(6, folly::ByteRange{});
          insertStmt.bind(7, static_cast<uint64_t>(isRestricted ? 1 : 0));
          insertStmt.bind(8, static_cast<uint64_t>(99));
          insertStmt.step();
        };

    insertEntry("invalid_restricted", 2, true);
    insertEntry("invalid_unrestricted", 3, false);
  });

  auto migratedStore = std::make_unique<SqliteTreeStore>(std::move(db));
  migratedStore->createTableIfNonExisting();
  migratedStore->loadCounters();

  auto loaded = migratedStore->loadTree(kRootNodeId);
  const auto& restricted = loaded.entries()->at("invalid_restricted");
  EXPECT_TRUE(*restricted.isRestricted());
  EXPECT_EQ(getAclRootState(restricted), AclRootState::RestrictedAclRoot);

  const auto& unrestricted = loaded.entries()->at("invalid_unrestricted");
  EXPECT_FALSE(*unrestricted.isRestricted());
  EXPECT_EQ(getAclRootState(unrestricted), AclRootState::Unknown);
}

} // namespace facebook::eden
