/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <cstdint>

#include <folly/CachelinePadded.h>
#include <folly/ClockGettimeWrappers.h>
#include <folly/Singleton.h>
#include <folly/SpinLock.h>
#include <folly/ThreadLocal.h>
#include <folly/io/async/Request.h>
#include <folly/logging/xlog.h>

#include "eden/fs/utils/IDGen.h"

namespace facebook {
namespace eden {

struct CompactTracePoint {
  // Holds nanoseconds since the epoch
  std::chrono::nanoseconds timestamp;
  // Opaque identifier for the entire trace - used to associate this
  // tracepoint with other tracepoints across an entire request
  uint64_t traceId;
  // Opaque identifier for this "block" where a block is some logical
  // piece of work with a well-defined start and stop point
  uint64_t blockId;
  // Opaque identifer for the parent block from which the current
  // block was constructed - used to create causal relationships
  // between blocks
  uint64_t parentBlockId;
  // The name of the block, only set on the tracepoint starting the
  // block, must point to a statically allocated cstring
  const char* name;
  // Flags indicating whether this block is starting, stopping, or neither
  uint8_t start : 1;
  uint8_t stop : 1;
};

// It's nice for each tracepoint to fit inside a single cache line
static_assert(sizeof(CompactTracePoint) <= 64);

namespace detail {
class ThreadLocalTracePoints {
  // CompactTracePoints are currently 48 bytes each, so this is 768 KB
  // per thread
  static constexpr size_t kBufferPoints = 16 * 1024;

 public:
  ThreadLocalTracePoints() = default;
  ~ThreadLocalTracePoints() {
    flush();
  }

  void flush();

  FOLLY_ALWAYS_INLINE void trace(
      uint64_t traceId,
      uint64_t blockId,
      uint64_t parentBlockId,
      const char* name,
      bool start,
      bool stop) {
    auto state = state_.lock();
    auto& tp = state->tracePoints_[state->currNum_++ % kBufferPoints];
    tp.traceId = traceId;
    tp.blockId = blockId;
    tp.parentBlockId = parentBlockId;
    tp.name = name;
    tp.start = start;
    tp.stop = stop;
    tp.timestamp = std::chrono::nanoseconds(
        folly::chrono::clock_gettime_ns(CLOCK_MONOTONIC));
  }

 private:
  struct State {
    size_t currNum_{0};
    std::array<CompactTracePoint, kBufferPoints> tracePoints_;
  };

  folly::Synchronized<State, folly::SpinLock> state_;
};

class TraceRequestData : public folly::RequestData {
 public:
  bool hasCallback() override {
    return false;
  }

  uint64_t traceId{0};
  uint64_t blockId{0};
};

extern folly::RequestToken tracingToken;

class Tracer {
 public:
  static TraceRequestData& getRequestData() {
    auto context = folly::RequestContext::get();
    XCHECK(context != nullptr);

    const auto& token = tracingToken;
    if (FOLLY_UNLIKELY(!context->hasContextData(token))) {
      auto reqData = std::make_unique<TraceRequestData>();
      context->setContextData(token, std::move(reqData));
    }
    folly::RequestData* data = context->getContextData(token);
    return *static_cast<TraceRequestData*>(data);
  }

  ThreadLocalTracePoints& getThreadLocalTracePoints() {
    return *tltp_;
  }

  std::vector<CompactTracePoint> getAllTracepoints();

  bool isEnabled() noexcept {
    return enabled_->load(std::memory_order_acquire);
  }

  void enable() noexcept {
    enabled_->store(true, std::memory_order_release);
  }

  void disable() noexcept {
    enabled_->store(false, std::memory_order_release);
  }

 private:
  friend class ThreadLocalTracePoints;
  struct Tag {};

  folly::CachelinePadded<std::atomic<bool>> enabled_{false};
  folly::ThreadLocal<ThreadLocalTracePoints, Tag, folly::AccessModeStrict>
      tltp_;
  // This is written to only when a thread dies and when
  // getAllTracepoints is invoked, though the latter will leave it
  // empty. As long as threads aren't continuously being created and
  // destroyed while tracing is on, this shouldn't grow large
  folly::Synchronized<std::vector<CompactTracePoint>> tracepoints_;
};

extern Tracer globalTracer;

} // namespace detail

/*
 * By default tracing is disabled, and TraceBlocks are very cheap
 * (single digit nanosecond overheads). When enabled, constructing and
 * destructing a TraceBlock costs ~150 ns.
 */
inline void enableTracing() {
  detail::globalTracer.enable();
}

inline void disableTracing() {
  detail::globalTracer.disable();
}

/*
 * This will gather all recorded tracepoints across all threads and
 * return them in timestamp order. Note that this is destructive -
 * repeated calls will not return previously returned tracepoints
 */
inline std::vector<CompactTracePoint> getAllTracepoints() {
  return detail::globalTracer.getAllTracepoints();
}

/*
 * TraceBlocks demark sections of eden's execution so we can analyze
 * the behavior of a request in a fine-grained fashion.

 * Create a TraceBlock by constructing it with a name (typically
 * identifying the operation it represents). When the TraceBlock is
 * destructed or the close() method is invoked, a tracepoint
 * indicating that the operation has completed is written. Take care
 * when interacting with futures to be sure that a TraceBlock lives as
 * long as the entire asynchronous operation.

 * TraceBlocks can be nested by creating multiple TraceBlocks before
 * destroying or close()ing one.
 *
 * Creating the first TraceBlock of a * request (FUSE, thrift, or
 * otherwise) will allocate a traceId which will be used to
 * associate all the future TraceBlocks of the request.
 */
class TraceBlock {
 public:
  /**
   * This parameter should be a string literal since its address is
   * stored in the trace point
   */
  template <size_t size>
  explicit TraceBlock(const char (&name)[size]) {
    if (detail::globalTracer.isEnabled()) {
      blockId_ = generateUniqueID();
      auto& reqData = detail::Tracer::getRequestData();
      if (!reqData.traceId) {
        reqData.traceId = generateUniqueID();
      }

      parentBlockId_ = reqData.blockId;
      detail::globalTracer.getThreadLocalTracePoints().trace(
          reqData.traceId,
          blockId_,
          parentBlockId_,
          name,
          /* start = */ true,
          /* stop = */ false);
      reqData.blockId = blockId_;
    }
  }

  TraceBlock(const TraceBlock&) = delete;
  TraceBlock& operator=(const TraceBlock&) = delete;
  TraceBlock(TraceBlock&& other) noexcept {
    blockId_ = other.blockId_;
    parentBlockId_ = other.parentBlockId_;
    other.blockId_ = 0;
  }
  TraceBlock& operator=(TraceBlock&& other) {
    close();
    blockId_ = other.blockId_;
    parentBlockId_ = other.parentBlockId_;
    other.blockId_ = 0;
    return *this;
  }

  ~TraceBlock() {
    close();
  }

  /**
   * Explicitly end the TraceBlock before the destructor
   */
  void close() {
    if (blockId_) {
      auto& reqData = detail::Tracer::getRequestData();
      detail::globalTracer.getThreadLocalTracePoints().trace(
          reqData.traceId,
          blockId_,
          parentBlockId_,
          nullptr,
          /* start = */ false,
          /* stop = */ true);
      reqData.blockId = parentBlockId_;
      blockId_ = 0;
    }
  }

 private:
  uint64_t blockId_{0};
  uint64_t parentBlockId_{0};
};

} // namespace eden
} // namespace facebook
