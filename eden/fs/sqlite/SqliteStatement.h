/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/String.h>
#include <sqlite3.h>

#include "eden/fs/sqlite/SqliteConnection.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
/**
 * Represents the sqlite vm that will execute a SQL statement.
 * The class can only be created while holding a lock on the SqliteDatabase;
 * this is enforced by the compiler.  However, the statement class
 * doesn't take ownership of the lock (it is perfectly valid for multiple
 * statements to interleave step() calls) so care must be taken by the caller
 * to ensure that associated SqliteStatement instances are only accessed
 * while the lock object is held.
 */
class SqliteStatement {
 public:
  /**
   * Prepare to execute the statement described by the `query` parameter
   */
  SqliteStatement(LockedSqliteConnection& db, folly::StringPiece sql);

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
  SqliteStatement(
      LockedSqliteConnection& db,
      Arg1&& first,
      Arg2&& second,
      Args&&... args)
      : SqliteStatement(
            db,
            folly::to<std::string>(
                std::forward<Arg1>(first),
                std::forward<Arg2>(second),
                std::forward<Args>(args)...)) {}

  SqliteStatement(const SqliteStatement&) = delete;
  SqliteStatement& operator=(const SqliteStatement&) = delete;

  SqliteStatement(SqliteStatement&&) = default;
  SqliteStatement& operator=(SqliteStatement&&) = default;

  /**
   * Make a single step in executing the statement.
   * For queries that return results, returns true if this step yielded a
   * data row.  It is then valid to use the `columnXXX` methods to access
   * the column data.
   * When the end of the result set is reached (or for queries such as
   * UPDATE or schema modifying queries), false is returned.
   * An exception is thrown on error.
   */
  bool step();

  /**
   * Bind a stringy parameter to a prepared statement placeholder.
   * Parameters are 1-based, with the first parameter having paramNo==1.
   * Throws an exception on error.
   * The bindType specifies a destructor function that sqlite will call
   * when it no longer needs to reference the value.  This defaults to
   * STATIC which means that sqlite will not try to free the value.
   * If the blob parameter references memory that will be invalidated
   * between the time that `bind` is called and the statement is
   * destroyed, you specified SQLITE_TRANSIENT for the `bindType`
   * parameter to request that sqlite make a copy before `bind`
   * returns. */
  void bind(
      size_t paramNo,
      folly::StringPiece blob,
      void (*bindType)(void*) = SQLITE_STATIC);

  /**
   * Identical to the StringPiece variant of `bind` defined above,
   * but accepts a ByteRange parameter instead
   */
  void bind(
      size_t paramNo,
      folly::ByteRange blob,
      void (*bindType)(void*) = SQLITE_STATIC);

  void bind(size_t paramNo, int64_t id);

  void bind(size_t paramNo, uint64_t id);

  void bind(size_t paramNo, uint32_t id);

  /** Reset SqliteStement and its bindings so it can be used again. */
  void reset();

  /**
   * Reference a blob column in the current row returned by the statement.
   * This is only valid to call once `step()` has returned true.  The
   * return value is invalidated by a subsequent `step()` call or by the
   * statement being destroyed.
   * Note that column indices are 0-based, with the first column having
   * colNo==0.
   * */
  folly::StringPiece columnBlob(size_t colNo) const;
  uint64_t columnUint64(size_t colNo) const;

  ~SqliteStatement();

 private:
  /** Small helper to safely narrow size_t to int */
  static inline int unsignedNoToInt(size_t no) {
    XDCHECK(no < std::numeric_limits<int>::max());
    return static_cast<int>(no);
  }

  /** Weak reference to the underlying database object */
  sqlite3* db_;
  /** The statement handle */
  sqlite3_stmt* stmt_;
};
} // namespace facebook::eden
