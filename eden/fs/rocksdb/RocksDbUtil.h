/*
 *  Copyright (c) 2016, Facebook, Inc.
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

/**
 * Returns an instance of a RocksDB that uses the specified directory for
 * storage. If there is an existing RocksDB at that path, it will be used.
 */
std::unique_ptr<rocksdb::DB> createRocksDb(folly::StringPiece dbPath);
}
}
