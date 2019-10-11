/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/Synchronized.h>
#include <folly/experimental/StringKeyedUnorderedMap.h>
#include "eden/fs/store/LocalStore.h"

namespace facebook {
namespace eden {

/** An implementation of LocalStore that stores values in memory.
 * Stored values remain in memory for the lifetime of the
 * MemoryLocalStore instance.
 * MemoryLocalStore is thread safe, allowing concurrent reads and
 * writes from any thread.
 * */
class MemoryLocalStore : public LocalStore {
 public:
  explicit MemoryLocalStore();
  void close() override;
  void clearKeySpace(KeySpace keySpace) override;
  void compactKeySpace(KeySpace keySpace) override;
  StoreResult get(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  bool hasKey(LocalStore::KeySpace keySpace, folly::ByteRange key)
      const override;
  void put(
      LocalStore::KeySpace keySpace,
      folly::ByteRange key,
      folly::ByteRange value) override;
  std::unique_ptr<LocalStore::WriteBatch> beginWrite(
      size_t bufSize = 0) override;

 private:
  folly::Synchronized<std::vector<folly::StringKeyedUnorderedMap<std::string>>>
      storage_;
};

} // namespace eden
} // namespace facebook
