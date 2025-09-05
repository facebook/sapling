/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/model/BlobAuxData.h"
#include "eden/fs/utils/ShardedLruCache.h"

using namespace facebook::eden;

ShardedLruCache<BlobAuxData> cache{1, 1};

std::vector<ObjectId> make_keys(size_t numKeys) {
  std::vector<ObjectId> keys;
  keys.reserve(numKeys);
  for (auto i = 0u; i < numKeys; i++) {
    keys.push_back(ObjectId::sha1(fmt::format("key{}", i)));
  }
  return keys;
}

void lru_cache_get(benchmark::State& state) {
  std::vector<ObjectId> keys = make_keys(state.range(1));

  if (state.thread_index() == 0) {
    auto numShards = state.range(0);
    cache = ShardedLruCache<BlobAuxData>{static_cast<size_t>(numShards), 128};

    for (auto& key : keys) {
      cache.store(key, BlobAuxData{kEmptySha1, kEmptyBlake3, 0});
    }
  }

  for (auto _ : state) {
    for (auto& key : keys) {
      cache.get(key);
    }
  }
}

void lru_cache_store(benchmark::State& state) {
  std::vector<ObjectId> keys = make_keys(state.range(1));

  if (state.thread_index() == 0) {
    auto numShards = state.range(0);
    cache = ShardedLruCache<BlobAuxData>{static_cast<size_t>(numShards), 128};
  }

  for (auto _ : state) {
    for (auto& key : keys) {
      cache.store(key, BlobAuxData{kEmptySha1, kEmptyBlake3, 0});
    }
  }
}

BENCHMARK(lru_cache_get)
    ->ThreadRange(1, 128)
    ->ArgNames({"num_shards", "num_keys"})
    ->RangeMultiplier(2)
    ->Ranges({{1, 32}, {1, 1024}});

BENCHMARK(lru_cache_store)
    ->ThreadRange(1, 128)
    ->ArgNames({"num_shards", "num_keys"})
    ->RangeMultiplier(2)
    ->Ranges({{1, 32}, {1, 1024}});

EDEN_BENCHMARK_MAIN();
