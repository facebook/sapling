/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <gtest/gtest.h>

#include <folly/executors/ThreadedExecutor.h>
#include <folly/futures/Future.h>

#include "eden/fs/tracing/Tracing.h"

using namespace facebook::eden;

namespace {
void ensureValidTracePoint(const CompactTracePoint& point) {
  EXPECT_NE(point.timestamp.count(), 0);
  EXPECT_NE(point.traceId, 0);
  EXPECT_NE(point.blockId, 0);
  if (point.start) {
    EXPECT_FALSE(point.stop);
    EXPECT_NE(point.name, nullptr);
  }
  if (point.stop) {
    EXPECT_FALSE(point.start);
    EXPECT_EQ(point.name, nullptr);
  }
}
void ensureValidTracePoints(
    const std::vector<CompactTracePoint>& points,
    size_t num) {
  ASSERT_EQ(points.size(), num);
  for (const auto& point : points) {
    ensureValidTracePoint(point);
  }
}

void ensureValidBlock() {
  auto points = getAllTracepoints();
  ensureValidTracePoints(points, 2);
  EXPECT_TRUE(points[0].start);
  EXPECT_TRUE(points[1].stop);
  EXPECT_EQ(points[0].traceId, points[1].traceId);
  EXPECT_EQ(points[0].blockId, points[1].blockId);
  EXPECT_STREQ(points[0].name, "my_block");
}
} // namespace

TEST(Tracing, records_block) {
  enableTracing();
  { TraceBlock block{"my_block"}; }

  ensureValidBlock();
}

TEST(Tracing, records_block_explicit_close) {
  enableTracing();
  {
    TraceBlock block{"my_block"};
    block.close();

    ensureValidBlock();
  }
}

TEST(Tracing, records_block_explicit_close_and_destroy) {
  enableTracing();
  {
    TraceBlock block{"my_block"};
    block.close();
  }

  ensureValidBlock();
}

TEST(Tracing, records_nested_block) {
  enableTracing();
  {
    TraceBlock block{"my_block"};
    TraceBlock block2{"my_block2"};
  }

  auto points = getAllTracepoints();
  ensureValidTracePoints(points, 4);
  EXPECT_TRUE(points[0].start);
  EXPECT_TRUE(points[1].start);
  EXPECT_TRUE(points[2].stop);
  EXPECT_TRUE(points[3].stop);
  for (auto i = 1; i < 4; ++i) {
    EXPECT_EQ(points[0].traceId, points[i].traceId);
  }
  EXPECT_EQ(points[0].blockId, points[3].blockId);
  EXPECT_EQ(points[1].blockId, points[2].blockId);
  EXPECT_NE(points[0].blockId, points[1].blockId);
  EXPECT_STREQ(points[0].name, "my_block");
  EXPECT_STREQ(points[1].name, "my_block2");
}

TEST(Tracing, records_traceId_across_futures) {
  enableTracing();
  TraceBlock block{"my_block"};
  folly::ThreadedExecutor executor;
  auto fut = folly::makeFuture(42).via(&executor).thenValue(
      [b = std::move(block)](auto /* unused */) {});
  fut.wait();

  ensureValidBlock();
}

TEST(Tracing, records_traceId_across_futures_no_early_tracepoint) {
  enableTracing();
  TraceBlock block{"my_block"};
  folly::ThreadedExecutor executor;
  auto fut = folly::makeFuture(42).via(&executor).thenValue(
      [b = std::move(block)](auto /* unused */) {
        EXPECT_EQ(getAllTracepoints().size(), 1)
            << "The block's end tracepoint should not have been logged yet";
      });
  fut.wait();
}

TEST(Tracing, does_not_record_if_disabled) {
  // Zeroes out all pending tracepoints from previous tests.
  (void)getAllTracepoints();

  disableTracing();
  { TraceBlock block{"my_block"}; }
  auto points = getAllTracepoints();
  ASSERT_EQ(0, points.size());
}
