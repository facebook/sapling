/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ActivityBuffer.h"

namespace facebook::eden {

ActivityBuffer::ActivityBuffer(uint32_t /* maxEvents */) {}

void ActivityBuffer::addEvent(InodeMaterializeEvent /* event */) {}

std::list<InodeMaterializeEvent> ActivityBuffer::getAllEvents() {
  return events_;
}

} // namespace facebook::eden
