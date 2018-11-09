/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/store/MemoryLocalStore.h"
#include <folly/String.h>
#include "eden/fs/store/StoreResult.h"
namespace facebook {
namespace eden {

using folly::StringPiece;

namespace {
class MemoryWriteBatch : public LocalStore::WriteBatch {
 public:
  explicit MemoryWriteBatch(MemoryLocalStore* store) : store_(store) {
    storage_.resize(LocalStore::KeySpace::End);
  }

  void put(
      LocalStore::KeySpace keySpace,
      folly::ByteRange key,
      folly::ByteRange value) override {
    storage_[keySpace][StringPiece(key)] = StringPiece(value).str();
  }

  void put(
      LocalStore::KeySpace keySpace,
      folly::ByteRange key,
      std::vector<folly::ByteRange> valueSlices) override {
    std::string value;
    for (const auto& slice : valueSlices) {
      value.append(reinterpret_cast<const char*>(slice.data()), slice.size());
    }
    put(keySpace, key, StringPiece(value));
  }

  void flush() override {
    for (size_t keySpace = 0; keySpace < storage_.size(); ++keySpace) {
      for (const auto& it : storage_[keySpace]) {
        store_->put(
            static_cast<LocalStore::KeySpace>(keySpace),
            folly::StringPiece(it.first),
            StringPiece(it.second));
      }
      storage_[keySpace].clear();
    }
  }

 private:
  MemoryLocalStore* store_;
  std::vector<folly::StringKeyedUnorderedMap<std::string>> storage_;
};
} // namespace

MemoryLocalStore::MemoryLocalStore(std::shared_ptr<ReloadableConfig> config)
    : LocalStore(std::move(config)) {
  storage_->resize(KeySpace::End);
}

void MemoryLocalStore::close() {}

void MemoryLocalStore::clearKeySpace(KeySpace keySpace) {
  (*storage_.wlock())[keySpace].clear();
}

void MemoryLocalStore::compactKeySpace(KeySpace) {}

StoreResult MemoryLocalStore::get(
    LocalStore::KeySpace keySpace,
    folly::ByteRange key) const {
  auto store = storage_.rlock();
  auto it = (*store)[keySpace].find(StringPiece(key));
  if (it == (*store)[keySpace].end()) {
    return StoreResult();
  }
  return StoreResult(std::string(it->second));
}

bool MemoryLocalStore::hasKey(
    LocalStore::KeySpace keySpace,
    folly::ByteRange key) const {
  auto store = storage_.rlock();
  auto it = (*store)[keySpace].find(StringPiece(key));
  return it != (*store)[keySpace].end();
}

void MemoryLocalStore::put(
    LocalStore::KeySpace keySpace,
    folly::ByteRange key,
    folly::ByteRange value) {
  (*storage_.wlock())[keySpace][StringPiece(key)] = StringPiece(value).str();
}

std::unique_ptr<LocalStore::WriteBatch> MemoryLocalStore::beginWrite(size_t) {
  return std::make_unique<MemoryWriteBatch>(this);
}

} // namespace eden
} // namespace facebook
