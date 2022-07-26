/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/ActivityBuffer.h"
#include <folly/portability/GTest.h>

using namespace facebook::eden;
namespace {

constexpr uint32_t kMaxBufLength = 10;

bool buffer_contains_event_with_ino(ActivityBuffer& buff, InodeNumber ino) {
  auto events = buff.getAllEvents();
  return std::find_if(events.begin(), events.end(), [&](auto event) {
           return event.ino.getRawValue() == ino.getRawValue();
         }) != events.end();
}

InodeTraceEvent create_inode_trace_event(InodeNumber ino) {
  return InodeTraceEvent(
      {std::chrono::system_clock::now(), std::chrono::steady_clock::now()},
      ino,
      InodeType::FILE,
      InodeEventType::MATERIALIZE,
      InodeEventProgress::END,
      std::chrono::microseconds(1000),
      "Test/File.txt");
}

} // namespace

TEST(ActivityBufferTest, initialize_buffer) {
  ActivityBuffer buff(kMaxBufLength);
}

TEST(ActivityBufferTest, buffer_zero_capacity) {
  ActivityBuffer buff(0);
  EXPECT_TRUE(buff.getAllEvents().empty());
  buff.addEvent(create_inode_trace_event(InodeNumber(1)));

  // Setting the ActivityBuffer max size to 0 means that events never get stored
  EXPECT_TRUE(buff.getAllEvents().empty());
  EXPECT_FALSE(buffer_contains_event_with_ino(buff, InodeNumber(1)));
}

TEST(ActivityBufferTest, add_events) {
  ActivityBuffer buff(kMaxBufLength);
  for (uint64_t i = 1; i <= kMaxBufLength; i++) {
    InodeTraceEvent event = create_inode_trace_event(InodeNumber(i));
    buff.addEvent(event);

    EXPECT_EQ(buff.getAllEvents().size(), i);
    EXPECT_TRUE(buffer_contains_event_with_ino(buff, event.ino));
  }

  // Check in this case all events are still stored and nothing was evicted yet
  for (uint64_t i = 1; i <= kMaxBufLength; i++) {
    EXPECT_TRUE(buffer_contains_event_with_ino(buff, InodeNumber(i)));
  }
}

TEST(ActivityBufferTest, add_exceed_capacity) {
  ActivityBuffer buff(kMaxBufLength);
  for (uint64_t i = 1; i <= kMaxBufLength + 1; i++) {
    InodeTraceEvent event = create_inode_trace_event(InodeNumber(i));
    buff.addEvent(event);
  }

  // Check that the buffer remained at its max size of kMaxBufLength and that
  // the oldest event (which had InodeNumber(1)) has been removed as expected
  EXPECT_EQ(buff.getAllEvents().size(), kMaxBufLength);
  EXPECT_FALSE(buffer_contains_event_with_ino(buff, InodeNumber(1)));
  for (uint64_t i = 2; i <= kMaxBufLength + 1; i++) {
    EXPECT_TRUE(buffer_contains_event_with_ino(buff, InodeNumber(i)));
  }
}
