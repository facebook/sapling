/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "PassThruInodes.h"
#include "eden/utils/LeaseCache.h"
#include <folly/FileUtil.h>
#include <folly/Singleton.h>
#include <wangle/concurrent/GlobalExecutor.h>

using namespace folly;

DEFINE_uint64(passthru_stat_cache_size,
              81920,
              "how many items to retain in stat cache");
DEFINE_uint64(passthru_stat_cache_ttl,
              10,
              "TTL for stat cache items, in seconds");
namespace facebook {
namespace eden {
namespace fusell {

namespace {
struct cached_stat {
  struct stat st;
  int err;
  time_t at;
};

struct lstatcache {
  eden::LeaseCache<folly::fbstring, cached_stat> cache;

  static folly::Future<std::shared_ptr<cached_stat>> doLstat(
      const folly::fbstring& name) {
    return via(wangle::getCPUExecutor().get())
        .then([=] {
          cached_stat info;
          if (::lstat(name.c_str(), &info.st) == 0) {
            info.err = 0;
          } else {
            info.err = errno;
          }
          time(&info.at);
          return std::make_shared<cached_stat>(info);
        });
  }

  lstatcache() : cache(FLAGS_passthru_stat_cache_size, doLstat) {}
};

folly::Singleton<lstatcache> statCache;

std::shared_ptr<lstatcache> getCache() {
#ifdef __APPLE__
  return statCache.get_weak().lock();
#else
  return statCache.try_get();
#endif
}
}

folly::Future<struct stat> cachedLstat(const folly::fbstring& name) {
  auto cache = getCache();
  return cache->cache.get(name).then([=](std::shared_ptr<cached_stat> info) {
    time_t now;
    time(&now);
    if (info->at + FLAGS_passthru_stat_cache_ttl < now) {
      cache->cache.erase(name);
      return cachedLstat(name);
    }

    if (info->err != 0) {
      throwSystemErrorExplicit(info->err);
    }

    return makeFuture(info->st);
  });
}
}
}
}

/* vim:ts=2:sw=2:et:
 */
