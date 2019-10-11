/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/container/EvictingCacheMap.h>
#include <folly/futures/Future.h>
#include <folly/futures/SharedPromise.h>

namespace facebook {
namespace eden {

template <typename KEY, typename VAL, typename HASH = std::hash<KEY>>
class LeaseCache {
 public:
  using ValuePtr = std::shared_ptr<VAL>;
  using FutureType = folly::Future<ValuePtr>;
  using SharedPromiseType = std::shared_ptr<folly::SharedPromise<ValuePtr>>;
  using FetchFunc = std::function<FutureType(const KEY& key)>;

 private:
  std::mutex lock_;
  folly::EvictingCacheMap<KEY, SharedPromiseType, HASH> cache_;
  FetchFunc fetcher_;

 public:
  LeaseCache(size_t maxSize, FetchFunc fetcher, size_t clearSize = 1)
      : cache_(maxSize, clearSize), fetcher_(fetcher) {}

  void set(const KEY& key, ValuePtr val) {
    std::lock_guard<std::mutex> g(lock_);
    auto entry = std::make_shared<typename SharedPromiseType::element_type>();
    entry->setValue(val);
    cache_.set(key, entry);
  }

  void erase(const KEY& key) {
    std::lock_guard<std::mutex> g(lock_);
    cache_.erase(key);
  }

  void setMaxSize(size_t size) {
    cache_.setMaxSize(size);
  }

  FutureType get(const KEY& key) {
    SharedPromiseType entry;

    {
      std::lock_guard<std::mutex> g(lock_);

      auto it = cache_.find(key);
      if (it != cache_.end()) {
        entry = it->second;
        return entry->getFuture();
      }

      entry = std::make_shared<typename SharedPromiseType::element_type>();
      cache_.set(key, entry);
    }

    auto future = entry->getFuture();

    fetcher_(key).thenTry(
        [entry](folly::Try<ValuePtr>&& t) { entry->setTry(std::move(t)); });

    return future;
  }

  bool exists(const KEY& key) {
    return cache_.exists(key);
  }
};

} // namespace eden
} // namespace facebook
