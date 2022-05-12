/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/portability/GTest.h>
#include <cstddef>
#include <memory>
#include <stdexcept>

#include "eden/fs/sqlite/PersistentSqliteStatement.h"
#include "eden/fs/sqlite/SqliteDatabase.h"
#include "eden/fs/sqlite/SqliteStatement.h"

namespace facebook::eden {

class SqliteTest : public testing::Test {
 public:
  SqliteTest() {}

  SqliteDatabase db{SqliteDatabase::inMemory};
};

TEST_F(SqliteTest, testStatement) {
  auto conn = db.lock();
  SqliteStatement stmt{conn, "SELECT 1"};
  ASSERT_TRUE(stmt.step());
  ASSERT_EQ(stmt.columnUint64(0), 1);
  ASSERT_FALSE(stmt.step());

  SqliteStatement bindStmt{conn, "SELECT ?"};
  bindStmt.bind(1, static_cast<int64_t>(10));
  ASSERT_TRUE(bindStmt.step());
  ASSERT_EQ(bindStmt.columnUint64(0), 10);
}

TEST_F(SqliteTest, testInvalidStatement) {
  auto conn = db.lock();
  ASSERT_THROW(
      (SqliteStatement{conn, "SELECT INVALID STATEMENT"}), std::runtime_error);
}

TEST_F(SqliteTest, testPersistentSqliteStatement) {
  std::optional<PersistentSqliteStatement> stmt = std::nullopt;
  {
    auto conn = db.lock();
    SqliteStatement(conn, R"(
    CREATE TABLE IF NOT EXISTS test
    (
        id INTEGER NOT NULL,
        PRIMARY KEY (id)
    )
        )")
        .step();
    stmt.emplace(conn, "INSERT INTO test (id) VALUES (?)");
  }

  // 1. insert a row with primary id = 1
  {
    auto conn = db.lock();
    auto exec = stmt->get(conn);
    exec->bind(1, static_cast<int64_t>(1));
    exec->step();
  }

  // 2. insert another row with primary id = 1, this should throw
  {
    auto conn = db.lock();
    auto exec = stmt->get(conn);
    exec->bind(1, static_cast<int64_t>(1));
    ASSERT_THROW(exec->step(), std::runtime_error);
  }

  // 3. insert a row with primary id = 2, this should still work
  {
    auto conn = db.lock();
    auto exec = stmt->get(conn);
    exec->bind(1, static_cast<int64_t>(2));
    exec->step();
  }
}
} // namespace facebook::eden
