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
  explicit StatementCache(LockedSqliteConnection& db)
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

SqliteDatabase::SqliteDatabase(AbsolutePathPiece path, DelayOpeningDB)
    : dbPath_(path.copy().value()), db_{}, cache_{nullptr} {}

SqliteDatabase::SqliteDatabase(std::string addr)
    : dbPath_(std::move(addr)), db_{} {
  openDb();
}

void SqliteDatabase::openDb() {
  auto lockedState = db_.wlock();
  switch (lockedState->status) {
    case SqliteDbStatus::CLOSED:
      throw std::runtime_error("Sqlite Db already closed before open.");
    case SqliteDbStatus::OPEN:
      throw std::runtime_error("Sqlite Db already opened before open.");
    case SqliteDbStatus::FAILED_TO_OPEN:
    case SqliteDbStatus::NOT_YET_OPENED:
      break;
  }
  sqlite3* db = nullptr;
  auto result = sqlite3_open(dbPath_.c_str(), &db);
  if (result != SQLITE_OK) {
    lockedState->status = SqliteDbStatus::FAILED_TO_OPEN;
    // sqlite3_close handles nullptr fine
    // @lint-ignore CLANGTIDY
    sqlite3_close(db);
    checkSqliteResult(nullptr, result);
  } else {
    lockedState->status = SqliteDbStatus::OPEN;
  }

  lockedState->db = db;

  cache_ = std::make_unique<StatementCache>(lockedState);
}

void SqliteDatabase::close() {
  auto db = db_.wlock();
  db->status = SqliteDbStatus::CLOSED;
  // We must clear the cached statement before closing the database. Otherwise
  // `sqlite3_close` will fail with `SQLITE_BUSY`. This rule applies to any
  // statement cache elsewhere too.
  cache_.reset();
  if (db->db) {
    sqlite3_close(db->db);
    db->db = nullptr;
  }
}

SqliteDatabase::~SqliteDatabase() {
  close();
}

LockedSqliteConnection SqliteDatabase::lock() {
  auto db = db_.wlock();
  switch (db->status) {
    case SqliteDbStatus::OPEN:
      break;
    case SqliteDbStatus::NOT_YET_OPENED:
      throw std::runtime_error(
          "the RocksDB local store has not yet been opened");
    case SqliteDbStatus::FAILED_TO_OPEN:
      throw std::runtime_error("the RocksDB local store failed to be opened");
    case SqliteDbStatus::CLOSED:
      throw std::runtime_error(
          "the RocksDB local store has already been closed");
  }
  return db;
}

void SqliteDatabase::transaction(
    const std::function<void(LockedSqliteConnection&)>& func) {
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

void SqliteDatabase::checkpoint() {
  if (auto conn = db_.tryWLock()) {
    XLOG(DBG6) << "Checkpoint thread acquired SQLite lock";
    try {
      int pnLog, pnCkpt;
      checkSqliteResult(
          conn->db,
          sqlite3_wal_checkpoint_v2(
              conn->db, nullptr, SQLITE_CHECKPOINT_FULL, &pnLog, &pnCkpt));
      XLOGF(
          DBG6,
          "Checkpoint saved. Size of frames: {}. Saved: {}",
          pnLog,
          pnCkpt);
    } catch (const std::exception&) {
      // Exception is logged in `checkSqliteResult`
    }
  } else {
    XLOG(DBG6) << "Checkpoint skipped: write lock is held by other threads";
  }
}
} // namespace facebook::eden
