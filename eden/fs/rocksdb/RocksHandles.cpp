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

#include <folly/logging/xlog.h>

using folly::StringPiece;
using rocksdb::ColumnFamilyDescriptor;
using rocksdb::ColumnFamilyHandle;
using rocksdb::DB;
using rocksdb::DBOptions;
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

  // If we wanted we could set options.info_log to control where RocksDB
  // log messages get sent.  By default they are written to a file named "LOG"
  // in the DB directory.
  // options.info_log = make_shared<CustomLogger>(InfoLogLevel::INFO_LEVEL);

  DB* dbRaw;
  columns.reserve(columnDescriptors.size());

  std::vector<ColumnFamilyHandle*> columnHandles;

  auto openDB = [&] {
    return DB::Open(
        options, dbPathStr, columnDescriptors, &columnHandles, &dbRaw);
  };

  // This will create any newly defined column families automatically,
  // so we needn't make any special migration steps here; just define
  // a new family and start to use it.
  // If we remove column families in the future this call will fail
  // and shout at us for not opening up the database with them defined.
  // We will need to do "something smarter" if we ever decide to perform
  // that kind of a migration.
  auto status = openDB();
  if (!status.ok()) {
    XLOG(ERR) << "Error opening RocksDB storage at " << dbPathStr << ": "
              << status.ToString();
    XLOG(ERR) << "Attempting to repair RocksDB " << dbPathStr;

    rocksdb::ColumnFamilyOptions unknownColumFamilyOptions;
    unknownColumFamilyOptions.OptimizeForPointLookup(8);
    unknownColumFamilyOptions.OptimizeLevelStyleCompaction();

    DBOptions dbOptions(options);
    status = RepairDB(
        dbPathStr, dbOptions, columnDescriptors, unknownColumFamilyOptions);
    if (!status.ok()) {
      throw std::runtime_error(folly::to<string>(
          "Unable to repair RocksDB at ", dbPathStr, ": ", status.ToString()));
    }

    columnHandles.clear();
    status = openDB();
    if (!status.ok()) {
      throw std::runtime_error(folly::to<string>(
          "Failed to open RocksDB at ",
          dbPathStr,
          " after repair attempt: ",
          status.ToString()));
    }
  }

  db.reset(dbRaw);
  for (auto h : columnHandles) {
    columns.emplace_back(h);
  }
}
} // namespace eden
} // namespace facebook
