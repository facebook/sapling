/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <deque>

#include <folly/Synchronized.h>

#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/telemetry/TraceBus.h"
#include "eden/fs/utils/PathFuncs.h"

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
  explicit ActivityBuffer(uint32_t maxEvents);

  ActivityBuffer(const ActivityBuffer&) = delete;
  ActivityBuffer(ActivityBuffer&&) = delete;
  ActivityBuffer& operator=(const ActivityBuffer&) = delete;
  ActivityBuffer& operator=(ActivityBuffer&&) = delete;

  /**
   * Adds a new TraceEvent to the ActivityBuffer. Evicts the oldest
   * event if the buffer was full (meaning maxEvents events were already stored
   * in the buffer).
   */
  void addEvent(TraceEvent event);

  /**
   * Returns an std::deque containing all TraceEvents stored in the
   * ActivityBuffer.
   */
  std::deque<TraceEvent> getAllEvents() const;

 private:
  uint32_t maxEvents_;
  folly::Synchronized<std::deque<TraceEvent>> events_;
};

} // namespace facebook::eden

#include "eden/fs/telemetry/ActivityBuffer-inl.h"
