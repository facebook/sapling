/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/lmdb/LMDBDatabase.h"

#include <folly/logging/xlog.h>

#include <sys/stat.h>

namespace facebook::eden {

// Maximum size of the LMDB database
// TODO: This should be configurable via EdenConfig
constexpr size_t LMDB_MAP_SIZE = 53687091200; // 50 GB

void checkLMDBResult(int mdb_status) {
  if (mdb_status != MDB_SUCCESS) {
    auto error = fmt::format(
        "lmdb error ({}): {}", mdb_status, mdb_strerror(mdb_status));
    XLOG(ERR) << error;
    throw std::runtime_error(error);
  }
}

void logLMDBError(int mdb_status) {
  if (mdb_status != MDB_SUCCESS) {
    auto error = fmt::format(
        "lmdb error ({}): {}", mdb_status, mdb_strerror(mdb_status));
    XLOG(ERR) << error;
    if (mdb_status == MDB_NOTFOUND) {
      errno = ENOENT;
    } else {
      errno = EINVAL;
    }
  }
}

LMDBDatabase::LMDBDatabase(AbsolutePathPiece path, DelayOpeningDB)
    : dbPath_(path.stringWithoutUNC()), conn_{} {}

LMDBDatabase::LMDBDatabase(AbsolutePathPiece path)
    : dbPath_(path.stringWithoutUNC()), conn_{} {
  openDb();
}

void LMDBDatabase::openDb() {
  auto conn = conn_.wlock();
  switch (conn->status) {
    case LMDBDbStatus::CLOSED:
      throw std::runtime_error("LMDB Db already closed before open.");
    case LMDBDbStatus::OPEN:
      throw std::runtime_error("LMDB Db already opened before open.");
    case LMDBDbStatus::FAILED_TO_OPEN:
    case LMDBDbStatus::NOT_YET_OPENED:
      break;
  }

  checkLMDBResult(mdb_env_create(&conn->mdb_env_));
  checkLMDBResult(mdb_env_set_mapsize(conn->mdb_env_, LMDB_MAP_SIZE));

  // MDB_NOLOCK : Don't do any locking. If concurrent access is anticipated, the
  // caller must manage all concurrency itself. For proper operation the caller
  // must enforce single-writer semantics, and must ensure that no readers are
  // using old transactions while a writer is active. The simplest approach is
  // to use an exclusive lock so that no readers may be active at all when a
  // writer begins.
  //
  // MDB_NOSYNC Don't flush system buffers to disk when committing a
  // transaction. This optimization means a system crash can corrupt the
  // database or lose the last transactions if buffers are not yet flushed to
  // disk. The risk is governed by how often the system flushes dirty buffers to
  // disk and how often mdb_env_sync() is called.
  //
  // However, if the filesystem preserves write order and the MDB_WRITEMAP flag
  // is not used, transactions exhibit ACI (atomicity, consistency, isolation)
  // properties and only lose D (durability). I.e. database integrity is
  // maintained, but a system crash may undo the final transactions.
  //
  // MDB_NOMETASYNC Flush system buffers to disk only once per transaction, omit
  // the metadata flush. Defer that until the system flushes files to disk, or
  // next non-MDB_RDONLY commit or mdb_env_sync(). This optimization maintains
  // database integrity, but a system crash may undo the last committed
  // transaction. I.e. it preserves the ACI (atomicity, consistency, isolation)
  // but not D (durability) database property.
  //
  // http://www.lmdb.tech/doc/group__mdb.html#ga32a193c6bf4d7d5c5d579e71f22e9340
  int flags = MDB_NOLOCK | MDB_NOSYNC | MDB_NOMETASYNC;

  auto result = mdb_env_open(conn->mdb_env_, dbPath_.c_str(), flags, 0664);

  if (result != MDB_SUCCESS) {
    XLOG(ERR) << "Failed to open lmdb db at" << dbPath_;
    conn->status = LMDBDbStatus::FAILED_TO_OPEN;
    mdb_env_close(conn->mdb_env_);
    checkLMDBResult(result);
  } else {
    XLOG(INFO) << "Opened lmdb db at" << dbPath_;
    conn->status = LMDBDbStatus::OPEN;
  }
}

void LMDBDatabase::close() {
  auto conn = conn_.wlock();
  conn->status = LMDBDbStatus::CLOSED;
  if (conn->mdb_env_) {
    mdb_env_close(conn->mdb_env_);
    conn->mdb_env_ = nullptr;
  }
  XLOG(INFO) << "Closed lmdb db at" << dbPath_;
}

LMDBDatabase::~LMDBDatabase() {
  close();
}

LockedLMDBConnection LMDBDatabase::lock() {
  auto conn = conn_.wlock();
  switch (conn->status) {
    case LMDBDbStatus::OPEN:
      break;
    case LMDBDbStatus::NOT_YET_OPENED:
      throw std::runtime_error(
          "the LMDBDatabase database has not yet been opened");
    case LMDBDbStatus::FAILED_TO_OPEN:
      throw std::runtime_error("the LMDBDatabase database failed to be opened");
    case LMDBDbStatus::CLOSED:
      throw std::runtime_error(
          "the LMDBDatabase database has already been closed");
  }
  return conn;
}

void LMDBDatabase::transaction(
    const std::function<void(LockedLMDBConnection&)>& func) {
  auto conn = lock();
  try {
    checkLMDBResult(mdb_txn_begin(conn->mdb_env_, nullptr, 0, &conn->mdb_txn_));
    checkLMDBResult(mdb_dbi_open(conn->mdb_txn_, nullptr, 0, &conn->mdb_dbi_));
    func(conn);
    checkLMDBResult(mdb_txn_commit(conn->mdb_txn_));
    mdb_dbi_close(conn->mdb_env_, conn->mdb_dbi_);
  } catch (const std::exception& ex) {
    mdb_txn_abort(conn->mdb_txn_);
    XLOG(WARN) << "LMDB transaction failed: " << ex.what();
    throw;
  }
}

void LMDBDatabase::checkpoint() {
  if (auto conn = conn_.tryWLock()) {
    XLOG(DBG6) << "Sync thread acquired LMDB lock";
    try {
      checkLMDBResult(mdb_env_sync(conn->mdb_env_, true));
      XLOGF(DBG6, "Sync performed");
    } catch (const std::exception&) {
      // Exception is logged in `checkLMDBResult`
    }
  } else {
    XLOG(DBG6) << "Sync skipped: write lock is held by other threads";
  }
}
} // namespace facebook::eden
