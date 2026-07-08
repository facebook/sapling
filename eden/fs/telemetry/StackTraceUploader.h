/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>
#include <string>

namespace facebook::eden {

/**
 * Uploads stack traces to Manifold and returns a URL for viewing them.
 */
class StackTraceUploader {
 public:
  static constexpr const char* kBucket = "edenfs-errors-stacktraces";
  static constexpr const char* kApiKey = "edenfs-errors-stacktraces-key";
  static constexpr const char* kClientIdentity = "edenfs_stack_trace_uploader";
  static constexpr int kUploadTimeoutSeconds = 3;
  static constexpr uint32_t kExpirationSeconds = 30 * 24 * 60 * 60; // 30 days

  /**
   * Generate a unique Manifold key for a stack trace.
   * Format: flat/{random_hex}
   */
  static std::string generateKey();

  /**
   * Convert a Manifold key to a viewable URL.
   */
  static std::string keyToUrl(const std::string& key);

  /**
   * Upload content to Manifold in background, return URL immediately.
   * If the upload queue is full, returns an error message instead.
   * Excess uploads beyond the pool's task queue are dropped with a debug log.
   */
  static std::string uploadToManifold(std::string content);
};

} // namespace facebook::eden
