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

bool buffer_contains_int(ActivityBuffer<int>& buff, int target) {
  auto contained_ints = buff.getAllEvents();
  return std::find(contained_ints.begin(), contained_ints.end(), target) !=
      contained_ints.end();
}

} // namespace

TEST(ActivityBufferTest, initialize_buffer) {
  ActivityBuffer<int> buff(kMaxBufLength);
}

TEST(ActivityBufferTest, buffer_zero_capacity) {
  ActivityBuffer<int> buff(0);
  EXPECT_TRUE(buff.getAllEvents().empty());
  buff.addEvent(1);

  // Setting the ActivityBuffer max size to 0 means that events never get stored
  EXPECT_TRUE(buff.getAllEvents().empty());
  EXPECT_FALSE(buffer_contains_int(buff, 1));
}

TEST(ActivityBufferTest, add_events) {
  ActivityBuffer<int> buff(kMaxBufLength);
  for (uint64_t i = 1; i <= kMaxBufLength; i++) {
    buff.addEvent(i);
    EXPECT_EQ(buff.getAllEvents().size(), i);
    EXPECT_TRUE(buffer_contains_int(buff, i));
  }

  // Check in this case all events are still stored and nothing was evicted yet
  for (uint64_t i = 1; i <= kMaxBufLength; i++) {
    EXPECT_TRUE(buffer_contains_int(buff, i));
  }
}

TEST(ActivityBufferTest, add_exceed_capacity) {
  ActivityBuffer<int> buff(kMaxBufLength);
  for (uint64_t i = 1; i <= kMaxBufLength + 1; i++) {
    buff.addEvent(i);
  }

  // Check that the buffer remained at its max size of kMaxBufLength and that
  // the oldest int (which was 1) has been removed as expected
  EXPECT_EQ(buff.getAllEvents().size(), kMaxBufLength);
  EXPECT_FALSE(buffer_contains_int(buff, 1));
  for (uint64_t i = 2; i <= kMaxBufLength + 1; i++) {
    EXPECT_TRUE(buffer_contains_int(buff, i));
  }
}
