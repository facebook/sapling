/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>

#include "eden/fs/lmdb/LMDBConnection.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {
// Given a lmdb result code, if the result was not successful
// (MDB_SUCCESS), format an error message and throw an exception.
void checkLMDBResult(int mdb_status);

// Given a lmdb result code, if the result was not successful
// (MDB_SUCCESS), format and log an error message and set errno. Does not
// throw an exception.
void logLMDBError(int mdb_status);

/** A helper class for managing a handle to a lmdb database. */
class LMDBDatabase {
 public:
  using DelayOpeningDB = folly::Unit;

  /**
   * Open a handle to the database at the specified path.
   * Will throw an exception if the database fails to open.
   * The database will be created if it didn't already exist.
   */
  explicit LMDBDatabase(AbsolutePathPiece path);

  /**
   * Constructs the LMDBDatabase object with out opening the database.
   * openDb must be called before any other method.
   */
  LMDBDatabase(AbsolutePathPiece path, DelayOpeningDB);

  /**
   * Open a handle to the database at the specified path.
   * Will throw an exception if the database fails to open.
   * The database will be created if it didn't already exist.
   */
  void openDb();

  // Not copyable...
  LMDBDatabase(const LMDBDatabase&) = delete;
  LMDBDatabase& operator=(const LMDBDatabase&) = delete;

  // But movable.
  LMDBDatabase(LMDBDatabase&&) = default;
  LMDBDatabase& operator=(LMDBDatabase&&) = default;

  /**
   * Close the handle.
   * This will happen implicitly at destruction but is provided
   * here for convenience.
   */
  void close();

  ~LMDBDatabase();

  /**
   * Obtain a locked database pointer suitable for passing
   * to the LMDBStatement class.
   */
  LockedLMDBConnection lock();

  /**
   * Executes a LMDB transaction. If the lambda body throws any error, the
   * transaction will be aborted.
   *
   * Example usage:
   *
   * ```
   * db_->transaction([](auto& conn) {
   *   mdb_del(conn->mdb_txn_, conn->mdb_dbi_, &key, nullptr);
   * };
   * ```
   */
  void transaction(const std::function<void(LockedLMDBConnection&)>& func);

  /**
   * Flush the data buffers to disk. Data is always written to disk when
   * mdb_txn_commit() is called, but the operating system may keep it buffered.
   * LMDB always flushes the OS buffers upon commit as well, unless the
   * environment was opened with MDB_NOSYNC or in part MDB_NOMETASYNC.
   *
   * This is a no-op if the conn_ lock is being held elsewhere.
   */
  void checkpoint();

 private:
  std::string dbPath_;

  folly::Synchronized<LMDBConnection> conn_;
};
} // namespace facebook::eden
