/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/TraceBus.h"
#include <folly/futures/Promise.h>
#include <folly/portability/GTest.h>
#include <atomic>
#include <thread>

using namespace std::literals;
using namespace facebook::eden;

TEST(TraceBusTest, construct_and_destruct) {
  auto bus = TraceBus<int>::create("bus", 10);
}

TEST(TraceBusTest, publish_reaches_subscriber) {
  folly::Promise<int> promise;
  auto future = promise.getFuture();

  auto bus = TraceBus<int>::create("bus", 10);
  auto handle = bus->subscribeFunction(
      "sub", [&](int event) { promise.setValue(event); });
  bus->publish(1234);

  EXPECT_EQ(1234, std::move(future).get(1000ms));
}

TEST(TraceBusTest, publishes_exceed_capacity) {
  std::vector<int> values;
  {
    auto bus = TraceBus<int>::create("bus", 1);
    auto handle =
        bus->subscribeFunction("sub", [&](int v) { values.push_back(v); });

    for (int i = 0; i < 100; ++i) {
      bus->publish(i);
    }
  }

  XCHECK_EQ(100ul, values.size());
  for (int i = 0; i < 100; ++i) {
    XCHECK_EQ(i, values[i]);
  }
}

TEST(TraceBusTest, unsubscribes_upon_exception) {
  int i = 0;

  {
    auto bus = TraceBus<int>::create("bus", 10);
    auto handle = bus->subscribeFunction("sub", [&](int v) {
      i += v;
      throw std::runtime_error{"boom"};
    });

    bus->publish(1);
    bus->publish(2);
  }

  XCHECK_EQ(1, i);
}

TEST(TraceBusTest, unsubscribe_in_arbitrary_order) {
  auto bus = TraceBus<folly::Unit>::create("bus", 10);
  int i = 0;
  auto h1 = bus->subscribeFunction("sub1", [&](folly::Unit) { i += 1; });
  auto h2 = bus->subscribeFunction("sub2", [&](folly::Unit) { i += 10; });
  auto h3 = bus->subscribeFunction("sub3", [&](folly::Unit) { i += 100; });

  bus->publish(folly::unit);
  bus->publish(folly::unit);
  h2.reset();
  bus->publish(folly::unit);
  h1.reset();
  bus->publish(folly::unit);
  h3.reset();
  bus->publish(folly::unit);
  bus.reset();

  // Given any of the subscriptions can have observed any events after they've
  // unsubscribed, we can't make assumptions about the value of i, but at least
  // ASAN and TSAN should observe any memory errors in unlinking from the linked
  // list.
}

TEST(TraceBusTest, unsubscribe_before_publish) {
  int i = 0;

  auto bus = TraceBus<int>::create("bus", 10);
  auto handle = bus->subscribeFunction("sub", [&](int v) { i += v; });
  bus->publish(1);
  handle.reset();
  bus->publish(2);
  bus.reset();

  // It's not guaranteed that unsubscribe will immediately prevent observation
  // of events.
  XCHECK(1 == i || i == 3) << i << " must be 1 or 3";
}

TEST(TraceBusTest, hasSubscriber) {
  auto bus = TraceBus<int>::create("bus", 10);
  ASSERT_FALSE(bus->hasSubscription());

  auto handle = bus->subscribeFunction("sub", [](auto) {});
  ASSERT_TRUE(bus->hasSubscription());

  handle.reset();
  bus->publish(1);

  // We need to wait for TraceBus's background thread to run and notice the
  // subscriber has been removed. This waits at most 10 seconds.
  auto deadline = std::chrono::steady_clock::now() + 10s;
  while (std::chrono::steady_clock::now() < deadline) {
    std::this_thread::yield();
    if (!bus->hasSubscription()) {
      break;
    }
  }
  ASSERT_FALSE(bus->hasSubscription());
}
