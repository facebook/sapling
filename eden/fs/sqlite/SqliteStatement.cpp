/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/sqlite/SqliteStatement.h"

#include "eden/fs/sqlite/SqliteDatabase.h"

namespace facebook::eden {
SqliteStatement::SqliteStatement(
    LockedSqliteConnection& db,
    folly::StringPiece query)
    : db_{db->db} {
  checkSqliteResult(
      db_,
      sqlite3_prepare_v3(
          db_,
          query.data(),
          unsignedNoToInt(query.size()),
          SQLITE_PREPARE_PERSISTENT,
          &stmt_,
          nullptr));
}

bool SqliteStatement::step() {
  XLOG(DBG9) << "Executing: " << sqlite3_sql(stmt_);
  auto result = sqlite3_step(stmt_);
  switch (result) {
    case SQLITE_ROW:
      return true;
    case SQLITE_DONE:
      reset();
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

void SqliteStatement::reset() {
  XLOG(DBG9) << "reset bindings";
  // We are intentionally not checking the result here since `sqlite3_reset`
  // will simply return the same result from `sqlite3_step` -- it should already
  // be handled.
  sqlite3_reset(stmt_);
  checkSqliteResult(db_, sqlite3_clear_bindings(stmt_));
}

folly::StringPiece SqliteStatement::columnBlob(size_t colNo) const {
  auto col = unsignedNoToInt(colNo);
  return folly::StringPiece(
      reinterpret_cast<const char*>(sqlite3_column_blob(stmt_, col)),
      sqlite3_column_bytes(stmt_, col));
}

uint64_t SqliteStatement::columnUint64(size_t colNo) const {
  return sqlite3_column_int64(stmt_, unsignedNoToInt(colNo));
}

SqliteStatement::~SqliteStatement() {
  sqlite3_finalize(stmt_);
}
}; // namespace facebook::eden
