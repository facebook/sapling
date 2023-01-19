/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/TaskTrace.h"
#include <folly/futures/Promise.h>
#include <folly/portability/GTest.h>

using namespace std::literals;
using namespace facebook::eden;

TEST(TaskTraceTest, subscription) {
  auto bus = TaskTraceEvent::getTraceBus();
  folly::Promise<std::string> promise;
  auto future = promise.getFuture();
  auto handle = bus->subscribeFunction("sub", [&](TaskTraceEvent event) {
    promise.setValue(std::string{event.name});
  });

  { TaskTraceBlock block{"hello"}; }

  EXPECT_EQ("hello", std::move(future).get(1000ms));
}
