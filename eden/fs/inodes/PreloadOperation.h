/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <chrono>
#include <optional>
#include <string>

#include <folly/Synchronized.h>

namespace facebook::eden {

/**
 * Thread-safe progress tracking for page cache preload operations.
 *
 * Supports both single-shot usage (known total) and incremental batch updates,
 * and tracks prefetch progress separately from preload progress.
 */
struct PreloadOperation {
  // Default constructor for incremental batch usage. The orchestrator stores
  // the batch count into remainingBatches up front, then calls
  // markBatchComplete() once per batch; the last call returns true so the
  // caller can finalize via markDone().
  PreloadOperation()
      : total(0),
        processed(0),
        remainingBatches(0),
        isDone(false),
        startTime(std::chrono::steady_clock::now()),
        durationMs(0),
        lastSweepProgress(-1),
        lastProgressAgeSec(0),
        prefetchTotal(0),
        prefetchProcessed(0),
        prefetchBytesTotal(0),
        prefetchBytesProcessed(0),
        prefetchDone(false) {}

  // Constructor for single-shot usage with known total.
  explicit PreloadOperation(int64_t totalFiles)
      : total(totalFiles),
        processed(0),
        remainingBatches(1),
        isDone(false),
        startTime(std::chrono::steady_clock::now()),
        durationMs(0),
        lastSweepProgress(-1),
        lastProgressAgeSec(0),
        prefetchTotal(0),
        prefetchProcessed(0),
        prefetchBytesTotal(0),
        prefetchBytesProcessed(0),
        prefetchDone(false) {}

  // Called when a batch completes. Returns true if this was the last batch.
  bool markBatchComplete() {
    return remainingBatches.fetch_sub(1, std::memory_order_acq_rel) == 1;
  }

  // Finalize the operation exactly once: stamp the elapsed duration, record
  // completionTime (so the cleanup sweep can reap the entry), then flip
  // isDone last so any reader that observes isDone==true also sees a valid
  // durationMs/completionTime. Call when the last batch/chunk finishes, or
  // immediately when there is no work to do (e.g. a zero-match glob).
  void markDone() {
    auto now = std::chrono::steady_clock::now();
    durationMs.store(
        std::chrono::duration_cast<std::chrono::milliseconds>(now - startTime)
            .count(),
        std::memory_order_release);
    completionTime.wlock()->emplace(now);
    isDone.store(true, std::memory_order_release);
  }

  std::atomic<int64_t> total;
  std::atomic<int64_t> processed;

  // Number of batches still being processed.
  std::atomic<int64_t> remainingBatches;

  std::atomic<bool> isDone;
  folly::Synchronized<std::optional<std::string>> error;
  std::chrono::steady_clock::time_point startTime;
  std::atomic<int64_t> durationMs;

  // Set when isDone becomes true. Used for lazy cleanup of stale entries.
  folly::Synchronized<std::optional<std::chrono::steady_clock::time_point>>
      completionTime;

  // Stranded-operation detection state, only touched by the cleanup sweep
  // (ServerState::cleanupStalePreloadProgress). lastSweepProgress is
  // processed + prefetchProcessed as of the previous sweep (-1 until the
  // first sweep); lastProgressAgeSec is the operation age (seconds since
  // startTime) at the last sweep that observed progress advancing. A
  // not-yet-done operation is presumed stranded - and reaped - only after a
  // full stranded-lifetime passes with no observed progress, so legitimately
  // slow preloads that are still moving are never reaped.
  std::atomic<int64_t> lastSweepProgress;
  std::atomic<int64_t> lastProgressAgeSec{0};

  // Prefetch phase tracking (fetching blobs from network).
  std::atomic<int64_t> prefetchTotal;
  std::atomic<int64_t> prefetchProcessed;
  std::atomic<int64_t> prefetchBytesTotal;
  std::atomic<int64_t> prefetchBytesProcessed;
  std::atomic<bool> prefetchDone;
};

} // namespace facebook::eden
