/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/store/ObjectCache.h"

namespace facebook::eden {

namespace {
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
  auto numObjects = 100000u;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);
  for (auto i = 0u; i < numObjects; i++) {
    auto object =
        std::make_shared<Object>(ObjectId::sha1(fmt::format("{}", i)));
    cache->insertSimple(object);
  }

  auto i = 0u;
  for (auto _ : st) {
    auto hash = ObjectId::sha1(fmt::format("{}", i));

    auto start = std::chrono::high_resolution_clock::now();
    auto res = cache->getSimple(hash);
    auto end = std::chrono::high_resolution_clock::now();

    benchmark::DoNotOptimize(res);

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    st.SetIterationTime(elapsed.count());

    i = (i + 1) % numObjects;
  }
}

BENCHMARK(getSimple)->UseManualTime();

void insertSimple(benchmark::State& st) {
  auto numObjects = 100000u;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);
  std::vector<std::shared_ptr<Object>> vec;
  vec.reserve(numObjects);
  for (auto i = 0u; i < numObjects; i++) {
    auto object =
        std::make_shared<Object>(ObjectId::sha1(fmt::format("{}", i)));
    vec.push_back(std::move(object));
  }

  auto i = 0u;
  for (auto _ : st) {
    auto start = std::chrono::high_resolution_clock::now();
    cache->insertSimple(vec[i]);
    auto end = std::chrono::high_resolution_clock::now();

    auto elapsed =
        std::chrono::duration_cast<std::chrono::duration<double>>(end - start);
    st.SetIterationTime(elapsed.count());

    i = (i + 1) % numObjects;
  }
}

BENCHMARK(insertSimple)->UseManualTime();

} // namespace
} // namespace facebook::eden

EDEN_BENCHMARK_MAIN();
