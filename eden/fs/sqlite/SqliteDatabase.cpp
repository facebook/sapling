/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/sqlite/SqliteDatabase.h"

#include <folly/logging/xlog.h>
#include "eden/fs/sqlite/PersistentSqliteStatement.h"

namespace facebook::eden {
struct SqliteDatabase::StatementCache {
  explicit StatementCache(SqliteDatabase::Connection& db)
      : beginTransaction{db, "BEGIN"},
        commitTransaction{db, "COMMIT"},
        rollbackTransaction{db, "ROLLBACK"} {}

  PersistentSqliteStatement beginTransaction;
  PersistentSqliteStatement commitTransaction;
  PersistentSqliteStatement rollbackTransaction;
};

void checkSqliteResult(sqlite3* db, int result) {
  if (result == SQLITE_OK) {
    return;
  }
  // Sometimes the db instance holds more useful context
  if (db) {
    auto error = fmt::format(
        "sqlite error ({}): {} {}",
        result,
        sqlite3_errstr(result),
        sqlite3_errmsg(db));
    XLOG(DBG6) << error;
    throw std::runtime_error(error);
  } else {
    // otherwise resort to a simpler number->string mapping
    auto error =
        fmt::format("sqlite error ({}): {}", result, sqlite3_errstr(result));
    XLOG(DBG6) << error;
    throw std::runtime_error(error);
  }
}

SqliteDatabase::SqliteDatabase(const char* addr) {
  sqlite3* db = nullptr;
  auto result = sqlite3_open(addr, &db);
  if (result != SQLITE_OK) {
    // sqlite3_close handles nullptr fine
    // @lint-ignore CLANGTIDY
    sqlite3_close(db);
    checkSqliteResult(nullptr, result);
  }
  db_ = db;
  auto conn = lock();
  cache_ = std::make_unique<StatementCache>(conn);
}

void SqliteDatabase::close() {
  auto db = db_.wlock();
  // We must clear the cached statement before closing the database. Otherwise
  // `sqlite3_close` will fail with `SQLITE_BUSY`. This rule applies to any
  // statement cache elsewhere too.
  cache_.reset();
  if (*db) {
    sqlite3_close(*db);
    *db = nullptr;
  }
}

SqliteDatabase::~SqliteDatabase() {
  close();
}

SqliteDatabase::Connection SqliteDatabase::lock() {
  return db_.wlock();
}

void SqliteDatabase::transaction(const std::function<void(Connection&)>& func) {
  auto conn = lock();
  try {
    cache_->beginTransaction.get(conn)->step();
    func(conn);
    cache_->commitTransaction.get(conn)->step();
  } catch (const std::exception& ex) {
    cache_->rollbackTransaction.get(conn)->step();
    XLOG(WARN) << "SQLite transaction failed: " << ex.what();
    throw;
  }
}
} // namespace facebook::eden
