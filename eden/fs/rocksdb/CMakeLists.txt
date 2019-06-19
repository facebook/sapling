# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB ROCKSDB_SRCS "*.cpp")
add_library(
  eden_rocksdb STATIC
    ${ROCKSDB_SRCS}
)
target_link_libraries(
  eden_rocksdb
  PUBLIC
    RocksDB::rocksdb
    Folly::folly
)
