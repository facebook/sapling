/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/MiniTracer.h"

#include <algorithm>
#include <atomic>
#include <mutex>
#include <vector>

#include <fmt/core.h>

namespace facebook::eden {

namespace {

// Format duration in human-readable form
std::string formatDuration(uint64_t nanoseconds) {
  static constexpr const char* units[] = {"ns", "us", "ms", "s"};
  double value = static_cast<double>(nanoseconds);
  size_t i;

  for (i = 0; value >= 1000.0 && i < std::size(units) - 1; ++i) {
    value /= 1000.0;
  }

  return value < 10.0
      ? fmt::format("{:.1f}{}", value, units[i])
      : fmt::format("{}{}", static_cast<uint64_t>(value + 0.5), units[i]);
}

// Convert time_point to nanoseconds since epoch for atomic storage
uint64_t toNanosSinceEpoch(std::chrono::steady_clock::time_point tp) {
  return static_cast<uint64_t>(
      std::chrono::duration_cast<std::chrono::nanoseconds>(
          tp.time_since_epoch())
          .count());
}

} // namespace

MiniTracer::MiniTracer()
    : startTimeNs_{toNanosSinceEpoch(std::chrono::steady_clock::now())} {}

MiniTracer::MiniTracer(uint64_t startTimeNs) : startTimeNs_{startTimeNs} {}

// Metadata for tracking spans with a given name using only atomic operations
struct MiniTracer::Span::Impl {
  // Counter of currently active (in-progress) spans
  std::atomic<uint64_t> activeSpanCount{0};

  // Total number of completed spans
  std::atomic<uint64_t> count{0};

  // Sum of all individual span durations (includes overlapping time)
  std::atomic<uint64_t> totalDurationNs{0};

  // Sum of non-overlapping wall clock time periods
  std::atomic<uint64_t> totalWallClockNs{0};

  // First time span was created
  uint64_t earliestStartNs{0};

  // Latest end time seen (stored as nanoseconds since epoch)
  std::atomic<uint64_t> latestEndNs{0};

  // Start time of current non-overlapping period (when activeSpanCount > 0)
  std::atomic<uint64_t> currentWallClockStartNs{0};
};

MiniTracer::Span::Span(std::shared_ptr<Impl> metadata, uint64_t startTimeNs)
    : metadata_{std::move(metadata)}, startTimeNs_{startTimeNs} {
  auto prevCount =
      metadata_->activeSpanCount.fetch_add(1, std::memory_order_acq_rel);
  if (prevCount == 0) {
    // We're the first active span - record wall clock start.
    metadata_->currentWallClockStartNs.store(
        startTimeNs_, std::memory_order_release);
  }
}

void MiniTracer::Span::end(uint64_t endTimeNs) {
  if (!metadata_) {
    return;
  }

  auto durationNs = endTimeNs - startTimeNs_;

  // Update total stats.
  metadata_->count.fetch_add(1, std::memory_order_relaxed);
  metadata_->totalDurationNs.fetch_add(durationNs, std::memory_order_relaxed);

  // Update latest time. This is racey, but doesn't matter.
  metadata_->latestEndNs.store(endTimeNs, std::memory_order_relaxed);

  auto wallStartNs =
      metadata_->currentWallClockStartNs.load(std::memory_order_acquire);

  // Decrement active span count and check if we're ending a non-overlapping
  // period.
  auto prevCount =
      metadata_->activeSpanCount.fetch_sub(1, std::memory_order_acq_rel);
  if (prevCount == 1) {
    // We were the last active span - tabulate wall clock duration.
    auto wallDurationNs = endTimeNs - wallStartNs;
    metadata_->totalWallClockNs.fetch_add(
        wallDurationNs, std::memory_order_relaxed);
  }

  // Mark this span as already ended so destructor won't record again.
  metadata_.reset();
}

MiniTracer::Span::~Span() {
  end(toNanosSinceEpoch(std::chrono::steady_clock::now()));
}

MiniTracer::Span MiniTracer::createSpanImpl(const char* name) {
  return createSpanImpl(
      name, toNanosSinceEpoch(std::chrono::steady_clock::now()));
}

MiniTracer::Span MiniTracer::createSpanImpl(
    const char* name,
    uint64_t startTimeNs) {
  std::shared_ptr<Span::Impl> metadata;

  // Try to find existing metadata with read lock.
  {
    std::shared_lock<std::shared_mutex> readLock{metadataMapMutex_};
    auto it = spanMetadata_.find(name);
    if (it != spanMetadata_.end()) {
      metadata = it->second;
    }
  }

  // Create metadata if not found (requires write lock).
  if (!metadata) {
    std::unique_lock<std::shared_mutex> writeLock{metadataMapMutex_};
    // Double-check after acquiring write lock.
    auto it = spanMetadata_.find(name);
    if (it != spanMetadata_.end()) {
      metadata = it->second;
    } else {
      metadata = std::make_shared<Span::Impl>();
      metadata->earliestStartNs = startTimeNs;
      spanMetadata_[name] = metadata;
    }
  }

  return Span{std::move(metadata), startTimeNs};
}

std::string MiniTracer::summarize() const {
  return summarize(toNanosSinceEpoch(std::chrono::steady_clock::now()));
}

std::string MiniTracer::summarize(uint64_t endTimeNs) const {
  std::shared_lock<std::shared_mutex> lock{metadataMapMutex_};

  if (spanMetadata_.empty()) {
    return "No spans recorded.\n";
  }

  // Calculate total time from tracer start to end time.
  uint64_t totalTimeNs = endTimeNs - startTimeNs_;

  // 1% threshold for filtering spans.
  uint64_t minWallTimeNs = totalTimeNs / 100;

  struct SpanInfo {
    std::string_view name;
    uint64_t count;
    uint64_t totalDurationNs;
    uint64_t wallClockNs;
    uint64_t earliestStartNs;
    uint64_t latestEndNs;
  };

  std::vector<SpanInfo> spans;
  spans.reserve(spanMetadata_.size());

  uint64_t globalLatestEnd = 0;

  for (const auto& [name, metadata] : spanMetadata_) {
    uint64_t count = metadata->count.load(std::memory_order_relaxed);
    uint64_t totalDurationNs =
        metadata->totalDurationNs.load(std::memory_order_relaxed);
    uint64_t wallClockNs =
        metadata->totalWallClockNs.load(std::memory_order_relaxed);
    uint64_t latestEndNs =
        metadata->latestEndNs.load(std::memory_order_relaxed);

    // Skip spans with wall time less than 1% of total time.
    if (wallClockNs < minWallTimeNs) {
      continue;
    }

    if (latestEndNs == 0) {
      // Skip spans with no end.
      continue;
    }

    globalLatestEnd = std::max(globalLatestEnd, latestEndNs);

    spans.push_back(
        SpanInfo{
            name,
            count,
            totalDurationNs,
            wallClockNs,
            metadata->earliestStartNs,
            latestEndNs});
  }

  std::sort(spans.begin(), spans.end(), [](const auto& a, const auto& b) {
    return a.earliestStartNs < b.earliestStartNs;
  });

  std::string result;
  constexpr int totalWidth = 80;

  uint64_t totalRange = std::max(globalLatestEnd - startTimeNs_, uint64_t{1});

  // Fixed column position where details should start (after the ASCII bars)
  constexpr size_t detailsColumn = 100;

  for (const auto& span : spans) {
    uint64_t startOffset = span.earliestStartNs - startTimeNs_;
    uint64_t endOffset = span.latestEndNs - startTimeNs_;
    uint64_t avgDurationNs = span.totalDurationNs / span.count;

    int startPos = startOffset * totalWidth / totalRange;
    int endPos = endOffset * totalWidth / totalRange;
    int spanWidth = std::max(1, endPos - startPos);

    double wallClockRatio = (endOffset > startOffset)
        ? static_cast<double>(span.wallClockNs) / (endOffset - startOffset)
        : 1.0;

    std::string line;
    line += std::string(startPos, ' ');
    line += "|+";
    line += formatDuration(startOffset);
    line += " ";

    // Draw more dashes for more wall-clock-heavy spans.
    int dashFrequency = wallClockRatio < 0.2 ? 4
        : wallClockRatio < 0.6               ? 3
        : wallClockRatio < 0.8               ? 2
                                             : 1;
    for (int i = 0; i < spanWidth; ++i) {
      line += (i % dashFrequency == 0) ? '-' : ' ';
    }

    line += " +";
    line += formatDuration(endOffset);
    line += "|";

    // Pad to fixed column position for details.
    while (line.length() < detailsColumn) {
      line += ' ';
    }

    line += fmt::format(
        " {} x{}, wall={}, sum={}, avg={}\n",
        span.name,
        span.count,
        formatDuration(span.wallClockNs),
        formatDuration(span.totalDurationNs),
        formatDuration(avgDurationNs));

    result += line;
  }

  return result;
}

} // namespace facebook::eden
