/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#pragma once
#include <folly/CppAttributes.h>

#include "eden/fs/rocksdb/RocksHandles.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/UnboundedQueueExecutor.h"

namespace facebook {
namespace eden {

class FaultInjector;

/** An implementation of LocalStore that uses RocksDB for the underlying
 * storage.
 */
class RocksDbLocalStore : public LocalStore {
 public:
  /**
   * The given FaultInjector must be valid during the lifetime of this
   * RocksDbLocalStore object.
   */
  explicit RocksDbLocalStore(
      AbsolutePathPiece pathToRocksDb,
      FaultInjector* FOLLY_NONNULL faultInjector,
      RocksDBOpenMode mode = RocksDBOpenMode::ReadWrite);
  ~RocksDbLocalStore();
  void close() override;
  void clearKeySpace(KeySpace keySpace) override;
  void compactKeySpace(KeySpace keySpace) override;
  StoreResult get(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  FOLLY_NODISCARD folly::Future<StoreResult> getFuture(
      KeySpace keySpace,
      folly::ByteRange key) const override;
  FOLLY_NODISCARD folly::Future<std::vector<StoreResult>> getBatch(
      KeySpace keySpace,
      const std::vector<folly::ByteRange>& keys) const override;
  bool hasKey(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  void put(
      LocalStore::KeySpace keySpace,
      folly::ByteRange key,
      folly::ByteRange value) override;
  std::unique_ptr<WriteBatch> beginWrite(size_t bufSize = 0) override;

  // Call RocksDB's RepairDB() function on the DB at the specified location
  static void repairDB(AbsolutePathPiece path);

  // Get the approximate number of bytes stored on disk for the
  // specified key space.
  uint64_t getApproximateSize(KeySpace keySpace) const;

 private:
  FaultInjector& faultInjector_;
  RocksHandles dbHandles_;
  mutable UnboundedQueueExecutor ioPool_;
};

} // namespace eden
} // namespace facebook
