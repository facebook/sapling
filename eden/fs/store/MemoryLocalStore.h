/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/Synchronized.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include "eden/fs/store/LocalStore.h"

namespace facebook::eden {

/** An implementation of LocalStore that stores values in memory.
 * Stored values remain in memory for the lifetime of the
 * MemoryLocalStore instance.
 * MemoryLocalStore is thread safe, allowing concurrent reads and
 * writes from any thread.
 * */
class MemoryLocalStore final : public LocalStore {
 public:
  explicit MemoryLocalStore();
  void open() override;
  void close() override;
  void clearKeySpace(KeySpace keySpace) override;
  void compactKeySpace(KeySpace keySpace) override;
  StoreResult get(KeySpace keySpace, folly::ByteRange key) const override;
  bool hasKey(KeySpace keySpace, folly::ByteRange key) const override;
  void put(KeySpace keySpace, folly::ByteRange key, folly::ByteRange value)
      override;
  std::unique_ptr<LocalStore::WriteBatch> beginWrite(
      size_t bufSize = 0) override;

 private:
  folly::Synchronized<std::vector<folly::StringKeyedUnorderedMap<std::string>>>
      storage_;
};

} // namespace facebook::eden
