/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>

#include "eden/fs/utils/RingBuffer.h"

namespace facebook::eden {

/**
 * ActivityBuffer is a fixed size buffer of stored EdenFS trace events whose
 * maximum size can be set when initialized. To be filled, an ActivityBuffer
 * must subscribe to some tracebus of events of the same type and add events
 * that it reads during the subscription. ActivityBuffer supports methods for
 * adding recent events (evicting old events in the process) as well as reading
 * all trace events currently stored in a thread safe manner.
 *
 * With the ActivityBuffer, we enable functionality for retroactive debugging of
 * expensive events in EdenFS by storing past event changes that users will be
 * able view at any time through retroactive versions of Eden's tracing CLI.
 */
template <typename TraceEvent>
class ActivityBuffer {
 public:
  explicit ActivityBuffer(size_t maxEvents);

  ActivityBuffer(const ActivityBuffer&) = delete;
  ActivityBuffer(ActivityBuffer&&) = delete;
  ActivityBuffer& operator=(const ActivityBuffer&) = delete;
  ActivityBuffer& operator=(ActivityBuffer&&) = delete;

  /**
   * Adds a new TraceEvent to the ActivityBuffer. Evicts the oldest
   * event if the buffer was full (meaning maxEvents events were already stored
   * in the buffer).
   */
  template <typename T>
  void addEvent(T&& event);

  /**
   * Returns a std::vector containing all TraceEvents stored in the
   * ActivityBuffer.
   */
  std::vector<TraceEvent> getAllEvents() const;

 private:
  folly::Synchronized<RingBuffer<TraceEvent>> events_;
};

template <typename TraceEvent>
ActivityBuffer<TraceEvent>::ActivityBuffer(size_t maxEvents)
    : events_{std::in_place, maxEvents} {}

template <typename TraceEvent>
template <typename T>
void ActivityBuffer<TraceEvent>::addEvent(T&& event) {
  events_.wlock()->push(std::forward<T>(event));
}

template <typename TraceEvent>
std::vector<TraceEvent> ActivityBuffer<TraceEvent>::getAllEvents() const {
  return events_.rlock()->toVector();
}

} // namespace facebook::eden
