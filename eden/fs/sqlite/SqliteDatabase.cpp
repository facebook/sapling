/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/sqlite/SqliteDatabase.h"

#include <folly/logging/xlog.h>
#include "eden/fs/sqlite/SqliteStatement.h"

namespace facebook::eden {
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
}

void SqliteDatabase::close() {
  auto db = db_.wlock();
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
    SqliteStatement(conn, "BEGIN TRANSACTION").step();
    func(conn);
    SqliteStatement(conn, "COMMIT").step();
  } catch (const std::exception& ex) {
    SqliteStatement(conn, "ROLLBACK").step();
    XLOG(WARN) << "SQLite transaction failed: " << ex.what();
    throw;
  }
}
} // namespace facebook::eden
