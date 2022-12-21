/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/store/ObjectCache.h"

namespace {

using namespace facebook::eden;

class Object {
 public:
  explicit Object(ObjectId hash) : hash_{std::move(hash)} {}

  const ObjectId& getHash() const {
    return hash_;
  }

  size_t getSizeBytes() const {
    return 1;
  }

 private:
  ObjectId hash_;
};

using SimpleObjectCache = ObjectCache<Object, ObjectCacheFlavor::Simple>;

void getSimple(benchmark::State& st) {
  size_t numObjects = 100000;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);

  std::vector<ObjectId> ids;
  ids.reserve(numObjects);

  for (size_t i = 0u; i < numObjects; ++i) {
    ids.push_back(ObjectId::sha1(fmt::to_string(i)));
    auto object = std::make_shared<Object>(ids[i]);
    cache->insertSimple(object);
  }

  size_t i = 0;
  for (auto _ : st) {
    benchmark::DoNotOptimize(cache->getSimple(ids[i]));

    if (++i == numObjects) {
      i = 0;
    }
  }
}
BENCHMARK(getSimple);

void insertSimple(benchmark::State& st) {
  size_t numObjects = 100000;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);
  std::vector<ObjectId> ids;
  ids.reserve(numObjects);
  std::vector<std::shared_ptr<Object>> vec;
  vec.reserve(numObjects);

  for (size_t i = 0; i < numObjects; ++i) {
    ids.push_back(ObjectId::sha1(fmt::to_string(i)));
    vec.push_back(std::make_shared<Object>(ids[i]));
  }

  size_t i = 0;
  for (auto _ : st) {
    cache->insertSimple(vec[i]);
    if (++i == numObjects) {
      i = 0;
    }
  }
  benchmark::DoNotOptimize(cache);
}
BENCHMARK(insertSimple);

} // namespace

EDEN_BENCHMARK_MAIN();
