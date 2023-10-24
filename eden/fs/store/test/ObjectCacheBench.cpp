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
  size_t getSizeBytes() const {
    return 1;
  }
};

using SimpleObjectCache = ObjectCache<Object, ObjectCacheFlavor::Simple>;

// 40 characters per line, 6 lines. 240 characters total.
std::string longObjectBase =
    "faceb00cdeadbeefc00010ff1badb0028badf00d"
    "faceb00cdeadbeefc00010ff1badb0028badf00d"
    "faceb00cdeadbeefc00010ff1badb0028badf00d"
    "faceb00cdeadbeefc00010ff1badb0028badf00d"
    "faceb00cdeadbeefc00010ff1badb0028badf00d"
    "faceb00cdeadbeefc00010ff1badb0028badf00d";

// a single character to mimic a very short Object ID
std::string shortObjectBase = "f";

void getSimple(benchmark::State& st, const std::string& objectBase) {
  size_t numObjects = 100000;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);

  std::vector<ObjectId> ids;
  ids.reserve(numObjects);

  for (size_t i = 0u; i < numObjects; ++i) {
    ids.emplace_back(ObjectId::sha1(fmt::to_string(i)).asString() + objectBase);
    auto object = std::make_shared<Object>();
    cache->insertSimple(ids[i], object);
  }

  size_t i = 0;
  for (auto _ : st) {
    benchmark::DoNotOptimize(cache->getSimple(ids[i]));

    if (++i == numObjects) {
      i = 0;
    }
  }
}

void shortGetSimple(benchmark::State& st) {
  getSimple(st, shortObjectBase);
}
BENCHMARK(shortGetSimple);

void longGetSimple(benchmark::State& st) {
  getSimple(st, longObjectBase);
}
BENCHMARK(longGetSimple);

void insertSimple(benchmark::State& st, const std::string& objectBase) {
  size_t numObjects = 100000;
  auto cache = SimpleObjectCache::create(40 * 1024 * 1024, 1);
  std::vector<ObjectId> ids;
  ids.reserve(numObjects);
  std::vector<std::shared_ptr<Object>> vec;
  vec.reserve(numObjects);

  for (size_t i = 0; i < numObjects; ++i) {
    ids.emplace_back(ObjectId::sha1(fmt::to_string(i)).asString() + objectBase);
    vec.push_back(std::make_shared<Object>());
  }

  size_t i = 0;
  for (auto _ : st) {
    cache->insertSimple(ids[i], vec[i]);
    if (++i == numObjects) {
      i = 0;
    }
  }
  benchmark::DoNotOptimize(cache);
}

void shortInsertSimple(benchmark::State& st) {
  insertSimple(st, shortObjectBase);
}
BENCHMARK(shortInsertSimple);

void longInsertSimple(benchmark::State& st) {
  insertSimple(st, longObjectBase);
}
BENCHMARK(longInsertSimple);

} // namespace

EDEN_BENCHMARK_MAIN();
