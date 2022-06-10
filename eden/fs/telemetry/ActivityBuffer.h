/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Synchronized.h>
#include "eden/fs/inodes/InodeNumber.h"
#include "eden/fs/inodes/InodeUtils.h"

namespace facebook::eden {

/**
 * Represents an event of when an inode materialized and the duration it took
 * for the event to take place
 */
class InodeMaterializeEvent {
 public:
  std::chrono::steady_clock::time_point timestamp;
  InodeNumber ino;
  InodeType inodeType;
  std::chrono::microseconds duration;
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
   * Adds a new InodeMaterializeEvent to the ActivityBuffer. Evicts the oldest
   * event if the buffer was full (meaning maxEvents events were already stored
   * in the buffer).
   */
  void addEvent(InodeMaterializeEvent event);

  /**
   * Returns an std::deque containing all InodeMaterializeEvents stored in the
   * ActivityBuffer.
   */
  std::deque<InodeMaterializeEvent> getAllEvents();

 private:
  uint32_t maxEvents_;
  folly::Synchronized<std::deque<InodeMaterializeEvent>> events_;
};

} // namespace facebook::eden
