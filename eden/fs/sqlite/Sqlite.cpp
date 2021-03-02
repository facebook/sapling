/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/sqlite/Sqlite.h"

#include <folly/logging/xlog.h>

using folly::StringPiece;
using folly::Synchronized;
using folly::to;
using std::string;

namespace facebook {
namespace eden {

// Given a sqlite result code, if the result was not successful
// (SQLITE_OK), format an error message and throw an exception.
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
    auto error =
        fmt::format("sqlite error ({}): {}", result, sqlite3_errstr(result));
    XLOG(DBG6) << error;
    // otherwise resort to a simpler number->string mapping
    throw std::runtime_error(error);
  }
}

SqliteDatabase::SqliteDatabase(const char* addr) {
  sqlite3* db = nullptr;
  auto result = sqlite3_open(addr, &db);
  if (result != SQLITE_OK) {
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

Synchronized<sqlite3*>::LockedPtr SqliteDatabase::lock() {
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

SqliteStatement::SqliteStatement(
    folly::Synchronized<sqlite3*>::LockedPtr& db,
    folly::StringPiece query)
    : db_{*db} {
  // debug logging to print every statment
  XLOG(DBG9) << query;
  checkSqliteResult(
      db_,
      sqlite3_prepare_v2(
          db_, query.data(), unsignedNoToInt(query.size()), &stmt_, nullptr));
}

bool SqliteStatement::step() {
  auto result = sqlite3_step(stmt_);
  switch (result) {
    case SQLITE_ROW:
      return true;
    case SQLITE_DONE:
      sqlite3_reset(stmt_);
      return false;
    default:
      checkSqliteResult(db_, result);
      folly::assume_unreachable();
  }
}

void SqliteStatement::bind(
    size_t paramNo,
    folly::StringPiece blob,
    void (*bindType)(void*)) {
  auto param = unsignedNoToInt(paramNo);
  XLOGF(DBG9, "?{} = {}", paramNo, blob);
  checkSqliteResult(
      db_,
      sqlite3_bind_blob64(
          stmt_, param, blob.data(), sqlite3_uint64(blob.size()), bindType));
}

void SqliteStatement::bind(
    size_t paramNo,
    folly::ByteRange blob,
    void (*bindType)(void*)) {
  auto sp = folly::StringPiece(blob);
  bind(paramNo, sp, bindType);
}

void SqliteStatement::bind(size_t paramNo, int64_t id) {
  XLOGF(DBG9, "?{} = {}", paramNo, id);
  checkSqliteResult(db_, sqlite3_bind_int64(stmt_, paramNo, id));
}

void SqliteStatement::bind(size_t paramNo, uint64_t id) {
  XLOGF(DBG9, "?{} = {}", paramNo, id);
  checkSqliteResult(
      db_, sqlite3_bind_int64(stmt_, unsignedNoToInt(paramNo), id));
}

void SqliteStatement::bind(size_t paramNo, uint32_t id) {
  XLOGF(DBG9, "?{} = {}", paramNo, id);
  checkSqliteResult(db_, sqlite3_bind_int(stmt_, unsignedNoToInt(paramNo), id));
}

StringPiece SqliteStatement::columnBlob(size_t colNo) const {
  auto col = unsignedNoToInt(colNo);
  return StringPiece(
      reinterpret_cast<const char*>(sqlite3_column_blob(stmt_, col)),
      sqlite3_column_bytes(stmt_, col));
}

uint64_t SqliteStatement::columnUint64(size_t colNo) const {
  return sqlite3_column_int64(stmt_, folly::to_signed(colNo));
}

SqliteStatement::~SqliteStatement() {
  sqlite3_finalize(stmt_);
}
} // namespace eden
} // namespace facebook
