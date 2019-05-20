/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/String.h>
#include <rocksdb/db.h>
#include <memory>
#include <string>

namespace facebook {
namespace eden {

enum class RocksDBOpenMode {
  ReadOnly,
  ReadWrite,
};

/**
 * This class is the holder of the database and column family handles
 * required to interact with our local rocksdb store.
 * RocksDB requires that we delete the column family handles prior
 * to deleting the DB so we need to manage the lifetime and
 * destruction order with this class.
 */
struct RocksHandles {
  std::unique_ptr<rocksdb::DB> db;

  // The order of these columns matches the descriptors passed
  // as column_descriptors to createRocksDb().
  std::vector<std::unique_ptr<rocksdb::ColumnFamilyHandle>> columns;

  /**
   * Note that the columns MUST be destroyed prior to the DB,
   * so we have a custom destructor for that purpose.
   */
  ~RocksHandles();

  /**
   * Returns an instance of a RocksDB that uses the specified directory for
   * storage. If there is an existing RocksDB at that path with
   * column_descriptors that match the requested set then it will be opened and
   * returned.  If there is no existing RocksDB at that location a new one will
   * be initialized using the requested column_descriptors.  Otherwise (an
   * existing RocksDB has mismatched column_descriptors) will throw an
   * exception.
   */
  RocksHandles(
      folly::StringPiece dbPath,
      RocksDBOpenMode mode,
      const rocksdb::Options& options,
      const std::vector<rocksdb::ColumnFamilyDescriptor>& columnDescriptors);

  void close();
};
} // namespace eden
} // namespace facebook
