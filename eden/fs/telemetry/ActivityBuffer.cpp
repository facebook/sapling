/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ActivityBuffer.h"
#include <folly/logging/xlog.h>

namespace facebook::eden {

ActivityBuffer::ActivityBuffer(uint32_t maxEvents) : maxEvents_(maxEvents) {}

void ActivityBuffer::addEvent(InodeMaterializeEvent event) {
  XLOG(DBG7) << fmt::format(
      "\nAdding InodeMaterializeEvent to ActivityBuffer\n{}", event);
  auto events = events_.wlock();
  events->push_back(std::move(event));
  if (events->size() > maxEvents_) {
    events->pop_front();
  }
}

std::deque<InodeMaterializeEvent> ActivityBuffer::getAllEvents() const {
  return *events_.rlock();
}

} // namespace facebook::eden
