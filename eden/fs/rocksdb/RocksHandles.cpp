/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/rocksdb/RocksHandles.h"

using folly::StringPiece;
using rocksdb::ColumnFamilyDescriptor;
using rocksdb::ColumnFamilyHandle;
using rocksdb::DB;
using rocksdb::Options;
using rocksdb::ReadOptions;
using rocksdb::Status;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

RocksHandles::~RocksHandles() {
  // MUST destroy the column handles first
  columns.clear();
  db.reset();
}

RocksHandles::RocksHandles(
    StringPiece dbPath,
    const std::vector<ColumnFamilyDescriptor>& columnDescriptors) {
  auto dbPathStr = dbPath.str();

  Options options;
  // Optimize RocksDB. This is the easiest way to get RocksDB to perform well.
  options.IncreaseParallelism();

  // Create the DB if it's not already present.
  options.create_if_missing = true;
  // Automatically create column families as we define new ones.
  options.create_missing_column_families = true;

  DB* dbRaw;
  columns.reserve(columnDescriptors.size());

  std::vector<ColumnFamilyHandle*> columnHandles;

  // This will create any newly defined column families automatically,
  // so we needn't make any special migration steps here; just define
  // a new family and start to use it.
  // If we remove column families in the future this call will fail
  // and shout at us for not opening up the database with them defined.
  // We will need to do "something smarter" if we ever decide to perform
  // that kind of a migration.
  auto status =
      DB::Open(options, dbPathStr, columnDescriptors, &columnHandles, &dbRaw);
  if (!status.ok()) {
    throw std::runtime_error(folly::to<string>(
        "Failed to open DB: ", dbPathStr, ": ", status.ToString()));
  }

  db.reset(dbRaw);
  for (auto h : columnHandles) {
    columns.emplace_back(h);
  }
}
} // namespace eden
} // namespace facebook
