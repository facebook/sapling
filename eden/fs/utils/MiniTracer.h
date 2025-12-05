/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <memory>
#include <shared_mutex>
#include <string>
#include <unordered_map>

namespace facebook::eden {

/**
 * A lightweight time tracking object for parallelized operations.
 *
 * MiniTracer aggregates start and end times for named spans, giving you an idea
 * of the total duration and "wall clock" duration for each span. Its purpose is
 * to help find the steps of a heavy parallelized operation (like "checkout")
 * that contribute most to wall clock latency.
 */
class MiniTracer {
 public:
  MiniTracer();
  explicit MiniTracer(uint64_t startTimeNs);

  /**
   * RAII sentinel object for tracking a named span.
   * Records start time on construction and end time on destruction.
   */
  class Span {
   public:
    ~Span();

    Span(const Span&) = delete;
    Span& operator=(const Span&) = delete;
    Span(Span&&) noexcept = default;
    Span& operator=(Span&&) noexcept = default;

    /**
     * End the span with a specified end time (in nanoseconds).
     * The span will not record again on destruction. Used by tests.
     */
    void end(uint64_t endTimeNs);

   private:
    friend class MiniTracer;

    struct Impl;

    Span(std::shared_ptr<Impl> metadata, uint64_t startTimeNs);

    std::shared_ptr<Impl> metadata_;
    uint64_t startTimeNs_;
  };

  /**
   * Create a named span that tracks time from construction to destruction.
   * Multiple spans with the same name can exist concurrently.
   *
   * Only accepts string literals to avoid heap allocation.
   */
  template <size_t N>
  [[nodiscard]] Span createSpan(const char (&name)[N]) {
    return createSpanImpl(name);
  }

  template <size_t N>
  [[nodiscard]] Span createSpan(const char (&name)[N], uint64_t startTimeNs) {
    return createSpanImpl(name, startTimeNs);
  }

  /**
   * Generate a human-readable summary of all tracked spans.
   */
  std::string summarize() const;
  std::string summarize(uint64_t endTimeNs) const;

  /**
   * Returns the elapsed duration since the tracer was created.
   */
  std::chrono::steady_clock::duration elapsed() const;

 private:
  [[nodiscard]] Span createSpanImpl(const char* name);
  [[nodiscard]] Span createSpanImpl(const char* name, uint64_t startTimeNs);

  mutable std::shared_mutex metadataMapMutex_;
  // Use string_view as key to avoid allocations when looking up by static
  // strings
  std::unordered_map<std::string_view, std::shared_ptr<Span::Impl>>
      spanMetadata_;
  uint64_t startTimeNs_;
};

} // namespace facebook::eden
