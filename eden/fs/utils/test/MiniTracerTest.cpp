/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/MiniTracer.h"

#include <folly/portability/GTest.h>

namespace facebook::eden {

TEST(MiniTracerTest, ControlledTimingTest) {
  // Use controlled time values for deterministic output.
  constexpr uint64_t kTrackerStart = 0;
  constexpr uint64_t kMillisecond = 1000000;

  MiniTracer tracker(kTrackerStart);

  // Create a non-overlapping long span that runs from 5ms to 15ms.
  auto seq =
      tracker.createSpan("sequential_slow", kTrackerStart + 5 * kMillisecond);
  seq.end(kTrackerStart + 15 * kMillisecond);

  // Create a non-overlapping short span that are far apart.
  auto seqFast1 =
      tracker.createSpan("sequential_fast", kTrackerStart + 10 * kMillisecond);
  seqFast1.end(kTrackerStart + 11 * kMillisecond);
  auto seqFast2 =
      tracker.createSpan("sequential_fast", kTrackerStart + 30 * kMillisecond);
  seqFast2.end(kTrackerStart + 31 * kMillisecond);

  // Create two overlapping spans
  auto overlap1 =
      tracker.createSpan("overlapping_op", kTrackerStart + 20 * kMillisecond);
  auto overlap2 =
      tracker.createSpan("overlapping_op", kTrackerStart + 30 * kMillisecond);
  overlap2.end(kTrackerStart + 35 * kMillisecond);
  overlap1.end(kTrackerStart + 40 * kMillisecond);

  // Get summary at 50ms mark
  auto summary = tracker.summarize(kTrackerStart + 50 * kMillisecond);

  const std::string expected =
      "          |+5.0ms -------------------- +15ms|                                                        sequential_slow x1, wall=10ms, sum=10ms, avg=10ms\n"
      "                    |+10ms -   -   -   -   -   -   -   -   -   -   -  +31ms|                         sequential_fast x2, wall=2.0ms, sum=2.0ms, avg=1.0ms\n"
      "                                        |+20ms ---------------------------------------- +40ms|       overlapping_op x2, wall=20ms, sum=25ms, avg=13ms\n";

  EXPECT_EQ(summary, expected);
}

} // namespace facebook::eden
