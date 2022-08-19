/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/sqlite/SqliteConnection.h"
#include "eden/fs/sqlite/SqliteStatement.h"

namespace facebook::eden {
/**
 * Wrapper around `SqliteStatement` to denote a cached `SqliteStatement` that is
 * used repeatedly. Using this type of statement to save the cost of
 * preparation. All cached `SqliteStatement` should be using this type to
 * prevent incorrect usage.
 *
 * All cached `PersistentSqliteStatement` must be destroyed before calling
 * `sqlite3_close` (i.e. destroying `SqliteDatabase`). Otherwise the connection
 * won't be closed, causing memory leak.
 */
class PersistentSqliteStatement {
 public:
  /** Prepare to execute the statement described by the `query` parameter */
  PersistentSqliteStatement(LockedSqliteConnection& db, folly::StringPiece sql)
      : stmt_(db, sql) {}

  /**
   * Join together the arguments as a single query string and prepare a
   * statement to execute them.
   *
   * Note: the `first` and `second` parameters are present to avoid a
   * delegation cycle for the otherwise amgiguous case of a single parameter.
   * It is desirable to do this because it saves an extraneous heap allocation
   * in the cases where the query string is known at compile time.
   */
  template <typename Arg1, typename Arg2, typename... Args>
  PersistentSqliteStatement(
      LockedSqliteConnection& db,
      Arg1&& first,
      Arg2&& second,
      Args&&... args)
      : PersistentSqliteStatement(
            db,
            folly::to<std::string>(
                std::forward<Arg1>(first),
                std::forward<Arg2>(second),
                std::forward<Args>(args)...)) {}

  PersistentSqliteStatement(const PersistentSqliteStatement&) = delete;
  PersistentSqliteStatement& operator=(const PersistentSqliteStatement&) =
      delete;

  PersistentSqliteStatement(PersistentSqliteStatement&&) = default;
  PersistentSqliteStatement& operator=(PersistentSqliteStatement&&) = default;

  struct Guard {
    explicit Guard(SqliteStatement& stmt) : stmt_(stmt) {}

    ~Guard() {
      stmt_.reset();
    }

    SqliteStatement& operator*() {
      return stmt_;
    }

    SqliteStatement* operator->() {
      return &stmt_;
    }

   private:
    SqliteStatement& stmt_;
  };

  /**
   * Obtain the cached statement for usage. The caller must be holding the
   * database lock in order to use the prepared statement. This function will
   * also take care of resetting the state of the given statement.
   */
  Guard get(LockedSqliteConnection&) & {
    return Guard{stmt_};
  }

 private:
  SqliteStatement stmt_;
};
} // namespace facebook::eden
