file(GLOB ROCKSDB_SRCS "*.cpp")
add_library(
  eden_rocksdb STATIC
    ${ROCKSDB_SRCS}
)
target_link_libraries(
  eden_rocksdb
  PUBLIC
    RocksDB::rocksdb
)
