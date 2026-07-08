/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/StackTraceUploader.h"

#include <fmt/core.h>
#include <folly/Random.h>
#include <folly/executors/CPUThreadPoolExecutor.h>
#include <folly/logging/xlog.h>

#include "eden/fs/rust/manifold_ffi/src/lib.rs.h"

namespace facebook::eden {

namespace {
constexpr size_t kUploadPoolSize = 4;

constexpr uint32_t kMaxQueuedUploads = 10;

folly::CPUThreadPoolExecutor& getUploadPool() {
  static folly::CPUThreadPoolExecutor pool(kUploadPoolSize);
  return pool;
}
} // namespace

std::string StackTraceUploader::generateKey() {
  auto hi = folly::Random::rand64();
  auto lo = folly::Random::rand64();
  return fmt::format("flat/{:016x}{:016x}", hi, lo);
}

std::string StackTraceUploader::keyToUrl(const std::string& key) {
  return fmt::format("manifold://{}/{}", kBucket, key);
}

std::string StackTraceUploader::uploadToManifold(std::string content) {
  auto key = generateKey();
  auto url = keyToUrl(key);

  // Drop if the upload queue is at capacity.
  auto& pool = getUploadPool();
  if (pool.getPendingTaskCount() >= kMaxQueuedUploads) {
    XLOG(DBG4) << "Dropping Manifold upload: queue full";
    return "Upload skipped: queue full";
  }

  pool.add([key = std::move(key), content = std::move(content)]() {
    try {
      manifold_write(
          kBucket,
          kApiKey,
          key,
          rust::Slice<const uint8_t>(
              reinterpret_cast<const uint8_t*>(content.data()), content.size()),
          kUploadTimeoutSeconds * 1000,
          kExpirationSeconds,
          kClientIdentity);
    } catch (const std::exception& ex) {
      XLOGF(
          WARN,
          "Failed to upload stack trace to Manifold key={} bucket={}: {}",
          key,
          kBucket,
          ex.what());
    }
  });

  return url;
}

} // namespace facebook::eden
