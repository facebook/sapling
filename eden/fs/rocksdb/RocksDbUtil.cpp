/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "RocksDbUtil.h"

using folly::StringPiece;
using rocksdb::DB;
using rocksdb::Options;
using rocksdb::ReadOptions;
using rocksdb::Status;
using std::string;
using std::unique_ptr;

namespace facebook {
namespace eden {

unique_ptr<DB> createRocksDb(StringPiece dbPath) {
  Options options;
  // Optimize RocksDB. This is the easiest way to get RocksDB to perform well.
  options.IncreaseParallelism();
  options.OptimizeLevelStyleCompaction();
  // Create the DB if it's not already present.
  options.create_if_missing = true;

  // Open DB.
  DB* db;
  Status status = DB::Open(options, dbPath.str(), &db);
  if (!status.ok()) {
    throw std::runtime_error(
        folly::to<string>("Failed to open DB: ", status.ToString()));
  }

  return unique_ptr<DB>(db);
}
}
}
