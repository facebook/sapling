/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/SqliteLocalStore.h"

#include <folly/String.h>
#include <folly/container/Array.h>
#include <folly/logging/xlog.h>

#include "eden/fs/sqlite/Sqlite.h"
#include "eden/fs/store/KeySpaces.h"
#include "eden/fs/store/StoreResult.h"

namespace facebook {
namespace eden {

using folly::ByteRange;
using folly::StringPiece;
using folly::Synchronized;
using folly::to;
using std::string;

namespace {

/**
 * Implements the write batching helper.
 * In an ideal world, we'd just start a transaction and have the WriteBatch
 * methods accumulate against that transaction, committing on flush.
 * To do that we'd either need to lock the underlying sqlite handle
 * for the lifetime of the WriteBatch, or open a separate database connection.
 * The latter might be interesting to explore if the cost of opening the
 * connection is cheap enough.
 * For now though, we batch up the incoming data and then send it to the
 * database in the flush method.
 */
class SqliteWriteBatch : public LocalStore::WriteBatch {
 public:
  explicit SqliteWriteBatch(SqliteDatabase& db) : db_(db) {
    buffer_.resize(LocalStore::KeySpace::End);
  }

  void put(LocalStore::KeySpace keySpace, ByteRange key, ByteRange value)
      override {
    buffer_[keySpace].emplace_back(
        StringPiece(key).str(), StringPiece(value).str());
  }

  void put(
      LocalStore::KeySpace keySpace,
      ByteRange key,
      std::vector<ByteRange> valueSlices) override {
    string value;
    for (auto& slice : valueSlices) {
      value.append(reinterpret_cast<const char*>(slice.data()), slice.size());
    }
    put(keySpace, key, StringPiece(value));
  }

  void flush() override {
    auto db = db_.lock();

    // Start a transaction for the flush operation
    SqliteStatement(db, "BEGIN").step();

    try {
      for (size_t i = 0; i < buffer_.size(); ++i) {
        auto& items = buffer_[i];
        if (items.empty()) {
          continue;
        }

        // See commentary in SqliteLocalStore::put re: `or ignore`
        SqliteStatement stmt(
            db,
            "insert or ignore into ",
            kKeySpaceRecords[i].name,
            " VALUES(?, ?)");

        for (const auto& item : items) {
          const auto& key = item.first;
          const auto& value = item.second;

          stmt.bind(1, key);
          stmt.bind(2, value);
          stmt.step();
        }
        items.clear();
      }

      SqliteStatement(db, "COMMIT").step();
    } catch (const std::exception&) {
      // Speculative rollback to make sure that we're not still in a
      // transaction if we bail out in the error path
      SqliteStatement(db, "ROLLBACK").step();
      throw;
    }
  }

 private:
  std::vector<std::vector<std::pair<string, string>>> buffer_;
  SqliteDatabase& db_;
};

} // namespace

SqliteLocalStore::SqliteLocalStore(AbsolutePathPiece pathToDb)
    : db_(SqliteDatabase(pathToDb)) {
  auto db = db_.lock();

  // Write ahead log for faster perf
  // https://www.sqlite.org/wal.html
  SqliteStatement(db, "PRAGMA journal_mode=WAL").step();

  for (const auto& ks : kKeySpaceRecords) {
    SqliteStatement(
        db,
        "CREATE TABLE IF NOT EXISTS ",
        ks.name,
        "(",
        "key BINARY NOT NULL,",
        "value BINARY NOT NULL,"
        "PRIMARY KEY (key)",
        ")")
        .step();
  }
}

void SqliteLocalStore::close() {
  db_.close();
}

void SqliteLocalStore::clearKeySpace(KeySpace keySpace) {
  auto db = db_.lock();

  SqliteStatement stmt(db, "delete from ", kKeySpaceRecords[keySpace].name);
  stmt.step();
}

void SqliteLocalStore::compactKeySpace(KeySpace) {}

StoreResult SqliteLocalStore::get(LocalStore::KeySpace keySpace, ByteRange key)
    const {
  auto db = db_.lock();

  SqliteStatement stmt(
      db,
      "select value from ",
      kKeySpaceRecords[keySpace].name,
      " where key = ?");

  // Bind the key; parameters are 1-based
  stmt.bind(1, key);

  if (stmt.step()) {
    // Return the result; columns are 0-based!
    return StoreResult(stmt.columnBlob(0).str());
  }

  // the key does not exist
  return StoreResult();
}

bool SqliteLocalStore::hasKey(LocalStore::KeySpace keySpace, ByteRange key)
    const {
  auto db = db_.lock();

  SqliteStatement stmt(
      db, "select 1 from ", kKeySpaceRecords[keySpace].name, " where key = ?");

  stmt.bind(1, key);
  return stmt.step();
}

void SqliteLocalStore::put(
    LocalStore::KeySpace keySpace,
    ByteRange key,
    ByteRange value) {
  auto db = db_.lock();

  SqliteStatement stmt(
      db,
      // TODO: we need `or ignore` otherwise we hit primary key violations
      // when running our integration tests.  This implies that we're
      // over-fetching and that we have a perf improvement opportunity.
      "insert or ignore into ",
      kKeySpaceRecords[keySpace].name,
      " VALUES(?, ?)");

  stmt.bind(1, key);
  stmt.bind(2, value);
  stmt.step();
}

std::unique_ptr<LocalStore::WriteBatch> SqliteLocalStore::beginWrite(size_t) {
  return std::make_unique<SqliteWriteBatch>(db_);
}

} // namespace eden
} // namespace facebook
