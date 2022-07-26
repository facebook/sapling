/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/service/gen-cpp2/eden_types.h"
#include "eden/fs/telemetry/TraceBus.h"
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

/**
 * Represents an inode state transition and the duration it took for the event
 * to occur. Currently the only supported transition is inode materialization
 * but we plan to support loads soon as well. This type extends the
 * TraceEventBase class so that events can be added to a tracebus for which an
 * ActivityBuffer subscribes to and stores events from.
 *
 * An inode materialization specifically refers to when a new version of
 * an inode's contents are saved in the overlay while before they referred
 * directly to a source control object. The duration we count for an inode
 * materialization consists of any time spent preparing/collecting file data,
 * writing the data to EdenFS's overlay, and materializing any parent inodes.
 */
struct InodeTraceEvent : TraceEventBase {
  InodeTraceEvent(
      TraceEventBase times,
      InodeNumber ino,
      InodeType inodeType,
      InodeEventType eventType,
      InodeEventProgress progress,
      std::chrono::microseconds duration,
      folly::StringPiece path);

  // Simple accessor that hides the internal memory representation of the trace
  // event's path. Note this could be just the base filename or it could be the
  // full path depending on how much was available and if the event has already
  // been added into the ActivityBuffer.
  std::string getPath() const {
    return path.get();
  }

  // Setter that allocates new memory on the heap and memcpy's a StringPiece's
  // data into the InodeTraceEvent's path attribute
  void setPath(folly::StringPiece stringPath);

  InodeNumber ino;
  InodeType inodeType;
  InodeEventType eventType;
  InodeEventProgress progress;
  std::chrono::microseconds duration;
  // Always null-terminated, and saves space in the trace event structure.
  std::shared_ptr<char[]> path;
};

/**
 * ActivityBuffer is a fixed size buffer of stored EdenFS events whose maximum
 * size can be set when iniatilized. While this buffer can currently only store
 * InodeMaterializeEvents, long term it is intended for the ActivityBuffer to
 * store many different kinds of events in EdenFS. The ActivityBuffer has
 * methods which allow for adding recent InodeMaterializeEvents as well as
 * reading all InodeMaterializeEvents currently stored in a thread safe manner.
 *
 * With the ActivityBuffer, we enable functionality for retroactive debugging of
 * expensive events in EdenFS by storing past event changes that users will be
 * able view at any time through retroactive versions of Eden's tracing CLI.
 */
class ActivityBuffer {
 public:
  explicit ActivityBuffer(uint32_t maxEvents);

  ActivityBuffer(const ActivityBuffer&) = delete;
  ActivityBuffer(ActivityBuffer&&) = delete;
  ActivityBuffer& operator=(const ActivityBuffer&) = delete;
  ActivityBuffer& operator=(ActivityBuffer&&) = delete;

  /**
   * Adds a new InodeTraceEvent to the ActivityBuffer. Evicts the oldest
   * event if the buffer was full (meaning maxEvents events were already stored
   * in the buffer).
   */
  void addEvent(InodeTraceEvent event);

  /**
   * Returns an std::deque containing all InodeTraceEvents stored in the
   * ActivityBuffer.
   */
  std::deque<InodeTraceEvent> getAllEvents() const;

 private:
  uint32_t maxEvents_;
  folly::Synchronized<std::deque<InodeTraceEvent>> events_;
};

} // namespace facebook::eden

namespace fmt {
template <>
struct formatter<facebook::eden::InodeTraceEvent> : formatter<std::string> {
  auto format(
      const facebook::eden::InodeTraceEvent& event,
      format_context& ctx) {
    std::string eventInfo = fmt::format(
        "Timestamp: {}\nInode Number: {}\nInode Type: {}\nDuration: {}",
        event.systemTime.time_since_epoch().count(),
        event.ino.getRawValue(),
        (event.inodeType == facebook::eden::InodeType::TREE ? "Tree" : "File"),
        event.duration.count());
    return formatter<std::string>::format(eventInfo, ctx);
  }
};
} // namespace fmt
