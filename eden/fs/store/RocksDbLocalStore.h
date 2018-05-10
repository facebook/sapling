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
#include "eden/fs/rocksdb/RocksHandles.h"
#include "eden/fs/store/LocalStore.h"
#include "eden/fs/utils/UnboundedQueueThreadPool.h"

namespace facebook {
namespace eden {

/** An implementation of LocalStore that uses RocksDB for the underlying
 * storage.
 */
class RocksDbLocalStore : public LocalStore {
 public:
  explicit RocksDbLocalStore(AbsolutePathPiece pathToRocksDb);
  ~RocksDbLocalStore();
  void close() override;
  StoreResult get(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  FOLLY_NODISCARD folly::Future<StoreResult> getFuture(
      KeySpace keySpace,
      folly::ByteRange key) const override;
  bool hasKey(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  void put(
      LocalStore::KeySpace keySpace,
      folly::ByteRange key,
      folly::ByteRange value) override;
  std::unique_ptr<WriteBatch> beginWrite(size_t bufSize = 0) override;

 private:
  RocksHandles dbHandles_;
  mutable UnboundedQueueThreadPool ioPool_;
};

} // namespace eden
} // namespace facebook
