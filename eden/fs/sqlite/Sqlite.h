/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/String.h>
#include <folly/Synchronized.h>
#include <sqlite3.h>
#include "eden/fs/utils/PathFuncs.h"
namespace facebook {
namespace eden {

// Given a sqlite result code, if the result was not successful
// (SQLITE_OK), format an error message and throw an exception.
void checkSqliteResult(sqlite3* db, int result);

/** A helper class for managing a handle to a sqlite database. */
class SqliteDatabase {
 public:
  /** Open a handle to the database at the specified path.
   * Will throw an exception if the database fails to open.
   * The database will be created if it didn't already exist.
   */
  explicit SqliteDatabase(AbsolutePathPiece path);

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
  folly::Synchronized<sqlite3*>::LockedPtr lock();

 private:
  folly::Synchronized<sqlite3*> db_{nullptr};
};

/** Represents the sqlite vm that will execute a SQL statement.
 * The class can only be created while holding a lock on the SqliteDatabase;
 * this is enforced by the compiler.  However, the statement class
 * doesn't take ownership of the lock (it is perfectly valid for multiple
 * statements to interleave step() calls) so care must be taken by the caller
 * to ensure that associated SqliteStatement instances are only accessed
 * while the lock object is held.
 */
class SqliteStatement {
 public:
  /** Prepare to execute the statement described by the `query` parameter */
  SqliteStatement(
      folly::Synchronized<sqlite3*>::LockedPtr& db,
      folly::StringPiece query);

  /** Join together the arguments as a single query string and prepare a
   * statement to execute them.
   * Note: the `first` and `second` parameters are present to avoid a delegation
   * cycle for the otherwise amgiguous case of a single parameter.  It is
   * desirable to do this because it saves an extraneous heap allocation in the
   * cases where the query string is known at compile time. */
  template <typename Arg1, typename Arg2, typename... Args>
  SqliteStatement(
      folly::Synchronized<sqlite3*>::LockedPtr& db,
      Arg1&& first,
      Arg2&& second,
      Args&&... args)
      : SqliteStatement(
            db,
            folly::to<std::string>(
                std::forward<Arg1>(first),
                std::forward<Arg2>(second),
                std::forward<Args>(args)...)) {}

  /** Make a single step in executing the statement.
   * For queries that return results, returns true if this step yielded a
   * data row.  It is then valid to use the `columnXXX` methods to access
   * the column data.
   * When the end of the result set is reached (or for queries such as
   * UPDATE or schema modifying queries), false is returned.
   * An exception is thrown on error.
   */
  bool step();

  /** Bind a stringy parameter to a prepared statement placeholder.
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

  /** Identical to the StringPiece variant of `bind` defined above,
   * but accepts a ByteRange parameter instead */
  inline void bind(
      size_t paramNo,
      folly::ByteRange blob,
      void (*bindType)(void*) = SQLITE_STATIC) {
    bind(paramNo, folly::StringPiece(blob), bindType);
  }

  /** Reference a blob column in the current row returned by the statement.
   * This is only valid to call once `step()` has returned true.  The
   * return value is invalidated by a subsequent `step()` call or by the
   * statement being destroyed.
   * Note that column indices are 0-based, with the first column having
   * colNo==0.
   * */
  folly::StringPiece columnBlob(size_t colNo) const;

  ~SqliteStatement();

 private:
  /** Weak reference to the underlying database object */
  sqlite3* db_;
  /** The statement handle */
  sqlite3_stmt* stmt_;
};

} // namespace eden
} // namespace facebook
