/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ActivityBuffer.h"
#include <folly/logging/xlog.h>

namespace facebook::eden {

InodeTraceEvent::InodeTraceEvent(
    TraceEventBase times,
    InodeNumber ino,
    InodeType inodeType,
    InodeEventType eventType,
    InodeEventProgress progress,
    std::chrono::microseconds duration,
    folly::StringPiece stringPath)
    : ino{ino},
      inodeType{inodeType},
      eventType{eventType},
      progress{progress},
      duration{duration} {
  systemTime = times.systemTime;
  monotonicTime = times.monotonicTime;
  setPath(stringPath);
}

void InodeTraceEvent::setPath(folly::StringPiece stringPath) {
  path.reset(new char[stringPath.size() + 1]);
  memcpy(path.get(), stringPath.data(), stringPath.size());
  path[stringPath.size()] = 0;
}

ActivityBuffer::ActivityBuffer(uint32_t maxEvents) : maxEvents_(maxEvents) {}

void ActivityBuffer::addEvent(InodeTraceEvent event) {
  XLOG(DBG7) << fmt::format(
      "\nAdding InodeTraceEvent to ActivityBuffer\n{}", event);
  auto events = events_.wlock();
  events->push_back(std::move(event));
  if (events->size() > maxEvents_) {
    events->pop_front();
  }
}

std::deque<InodeTraceEvent> ActivityBuffer::getAllEvents() const {
  return *events_.rlock();
}

} // namespace facebook::eden
