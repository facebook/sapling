/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <gtest/gtest_prod.h>
#include <atomic>
#include <memory>

#include <fmt/format.h>
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/utils/PathFuncs.h"

struct sqlite3;

namespace facebook::eden {
namespace overlay {
class OverlayDir;
class OverlayEntry;
} // namespace overlay
class SqliteStatement;
struct InodeNumber;

class SqliteTreeStoreNonEmptyError : public std::exception {
 public:
  explicit SqliteTreeStoreNonEmptyError(std::string&& str)
      : message_(folly::to<std::string>(
            "Attempting to operate on non-empty directory: ",
            str)) {}

  const char* what() const noexcept override {
    return message_.c_str();
  }

 private:
  std::string message_;
};

/**
 * An overlay backed by SQLite specializing in tree storage.
 */
class SqliteTreeStore {
 public:
  enum class SynchronousMode : uint8_t {
    Off = 0,
    Normal = 1,
  };

  explicit SqliteTreeStore(
      AbsolutePathPiece dir,
      SqliteTreeStore::SynchronousMode mode =
          SqliteTreeStore::SynchronousMode::Normal);

  explicit SqliteTreeStore(std::unique_ptr<SqliteDatabase> db);

  ~SqliteTreeStore();

  SqliteTreeStore(const SqliteTreeStore&) = delete;
  SqliteTreeStore& operator=(const SqliteTreeStore&) = delete;
  SqliteTreeStore(SqliteTreeStore&& other) = delete;
  SqliteTreeStore& operator=(SqliteTreeStore&& other) = delete;

  void close();

  /**
   * Create table and indexes if they are not already created. This function
   * will throw if it fails.
   */
  void createTableIfNonExisting();

  /**
   * Load the internal counters (inode and sequence_id) based on data in the
   * storage.
   */
  InodeNumber loadCounters();

  /**
   * Retrieve next available inode number
   */
  InodeNumber nextInodeNumber();

  /**
   * Save tree into storage
   */
  void saveTree(InodeNumber inodeNumber, overlay::OverlayDir&& odir);

  /**
   * Load tree from storage
   */
  overlay::OverlayDir loadTree(InodeNumber inode);

  /**
   * Remove the tree from the store and return it.
   */
  overlay::OverlayDir loadAndRemoveTree(InodeNumber inode);

  /**
   * Delete a tree from storage
   *
   * @throws if the tree being deleted is non-empty
   */
  void removeTree(InodeNumber inode);

  /**
   * Check if the given inode number exists in the storage.
   */
  bool hasTree(InodeNumber inode);

  /**
   * Add a child to the given parent
   */
  void addChild(
      InodeNumber parent,
      PathComponentPiece name,
      overlay::OverlayEntry entry);

  /**
   * Remove a child from the given parent
   */
  void removeChild(InodeNumber parent, PathComponentPiece childName);

  /**
   * Has the child for the given parent
   */
  bool hasChild(InodeNumber parent, PathComponentPiece childName);

  /**
   * Remove a child from the given parent
   *
   * @throws if renaming a tree and destination is non-empty
   */
  void renameChild(
      InodeNumber src,
      InodeNumber dst,
      PathComponentPiece srcName,
      PathComponentPiece dstName);

  std::unique_ptr<SqliteDatabase> takeDatabase();

  void maintenance() {
    db_->checkpoint();
  }

 private:
  FRIEND_TEST(SqliteTreeStoreTest, testRecoverInodeEntryNumber);

  struct StatementCache;

  /**
   * Private helper function to add a SQLite statement that inserts a row to the
   * inode table.
   */
  void insertInodeEntry(
      SqliteStatement& stmt,
      size_t index,
      InodeNumber parent,
      PathComponentPiece name,
      const overlay::OverlayEntry& entry);

  std::unique_ptr<SqliteDatabase> db_;

  std::unique_ptr<StatementCache> cache_;

  std::atomic_uint64_t nextEntryId_{0};

  std::atomic_uint64_t nextInode_{0};
};
} // namespace facebook::eden
