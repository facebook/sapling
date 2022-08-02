/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/logging/xlog.h>

namespace facebook::eden {

template <typename TraceEvent>
ActivityBuffer<TraceEvent>::ActivityBuffer(uint32_t maxEvents)
    : maxEvents_(maxEvents) {}

template <typename TraceEvent>
void ActivityBuffer<TraceEvent>::addEvent(TraceEvent event) {
  auto events = events_.wlock();
  events->push_back(std::move(event));
  if (events->size() > maxEvents_) {
    events->pop_front();
  }
}

template <typename TraceEvent>
std::deque<TraceEvent> ActivityBuffer<TraceEvent>::getAllEvents() const {
  return *events_.rlock();
}

} // namespace facebook::eden
