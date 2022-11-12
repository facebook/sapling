/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/rocksdb/RocksHandles.h"

#include <folly/logging/xlog.h>

#include "eden/fs/rocksdb/RocksException.h"

using rocksdb::ColumnFamilyDescriptor;
using rocksdb::ColumnFamilyHandle;
using rocksdb::DB;
using rocksdb::Options;
using rocksdb::Status;

namespace facebook::eden {

RocksHandles::~RocksHandles() {
  close();
}

void RocksHandles::close() {
  // MUST destroy the column handles first
  columns.clear();
  db.reset();
}

RocksHandles::RocksHandles(
    std::string_view dbPath,
    RocksDBOpenMode mode,
    const Options& options,
    const std::vector<ColumnFamilyDescriptor>& columnDescriptors) {
  auto dbPathStr = std::string{dbPath};
  DB* dbRaw;
  std::vector<ColumnFamilyHandle*> columnHandles;

  // This will create any newly defined column families automatically,
  // so we needn't make any special migration steps here; just define
  // a new family and start to use it.
  // If we remove column families in the future this call will fail
  // and shout at us for not opening up the database with them defined.
  // We will need to do "something smarter" if we ever decide to perform
  // that kind of a migration.
  Status status;
  if (mode == RocksDBOpenMode::ReadOnly) {
    status = DB::OpenForReadOnly(
        options, dbPathStr, columnDescriptors, &columnHandles, &dbRaw);
  } else {
    status =
        DB::Open(options, dbPathStr, columnDescriptors, &columnHandles, &dbRaw);
  }
  if (!status.ok()) {
    XLOG(ERR) << "Error opening RocksDB storage at " << dbPathStr << ": "
              << status.ToString();
    throw RocksException::build(
        status, "error opening RocksDB storage at", dbPathStr);
  }

  db.reset(dbRaw);
  columns.reserve(columnHandles.size());
  for (auto h : columnHandles) {
    columns.emplace_back(h);
  }
}
} // namespace facebook::eden
