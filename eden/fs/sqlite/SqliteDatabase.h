/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include <sqlite3.h>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
// Given a sqlite result code, if the result was not successful
// (SQLITE_OK), format an error message and throw an exception.
void checkSqliteResult(sqlite3* db, int result);

/** A helper class for managing a handle to a sqlite database. */
class SqliteDatabase {
 public:
  using Connection = folly::Synchronized<sqlite3*>::LockedPtr;

  constexpr static struct InMemory {
  } inMemory{};

  /** Open a handle to the database at the specified path.
   * Will throw an exception if the database fails to open.
   * The database will be created if it didn't already exist.
   */
  explicit SqliteDatabase(AbsolutePathPiece path)
      : SqliteDatabase(path.copy().c_str()) {}

  /**
   * Create a SQLite database in memory. It will throw an exception if the
   * database fails to open. This should be only used in testing.
   */
  explicit SqliteDatabase(InMemory) : SqliteDatabase(":memory:") {}

  // Not copyable...
  SqliteDatabase(const SqliteDatabase&) = delete;
  SqliteDatabase& operator=(const SqliteDatabase&) = delete;

  // But movable.
  SqliteDatabase(SqliteDatabase&&) = default;
  SqliteDatabase& operator=(SqliteDatabase&&) = default;

  /** Close the handle.
   * This will happen implicitly at destruction but is provided
   * here for convenience. */
  void close();

  ~SqliteDatabase();

  /** Obtain a locked database pointer suitable for passing
   * to the SqliteStatement class. */
  Connection lock();

  /**
   * Executes a SQLite transaction. If the lambda body throws any error, the
   * transaction will be rolled back. This function returns a boolean to
   * indicate whether the transaction is successfully committed.
   *
   * Example usage:
   *
   * ```
   * db_->transaction([](auto& conn) {
   *   SqliteStatement(conn, "SELECT * ...").step();
   *   SqliteStatement(conn, "INSERT INTO ...").step();
   * };
   * ```
   */
  void transaction(const std::function<void(Connection&)>& func);

  void checkpoint();

 private:
  struct StatementCache;

  explicit SqliteDatabase(const char* address);

  folly::Synchronized<sqlite3*> db_{nullptr};

  std::unique_ptr<StatementCache> cache_;
};
} // namespace facebook::eden
