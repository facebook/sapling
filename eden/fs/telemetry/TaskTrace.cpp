/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/telemetry/TaskTrace.h"

#include <folly/system/ThreadId.h>
#include <folly/system/ThreadName.h>
#include <chrono>

namespace facebook::eden {

namespace {
void publish(TaskTraceEvent&& event) {
  TaskTraceEvent::getTraceBus()->publish(std::move(event));
}
} // namespace

const std::shared_ptr<TraceBus<TaskTraceEvent>>& TaskTraceEvent::getTraceBus() {
  // Reserve 8 spots for each thread we can possibly run at the same time.
  static std::shared_ptr<TraceBus<TaskTraceEvent>> traceBus =
      TraceBus<TaskTraceEvent>::create(
          "task", std::thread::hardware_concurrency() * 8);
  return traceBus;
}

TaskTraceBlock::TaskTraceBlock(std::string_view task) {
  if (TaskTraceEvent::getTraceBus()->hasSubscription()) {
    name = std::move(task);
    threadName = folly::getCurrentThreadName().value_or("<unknown>");
    threadId = folly::getOSThreadID();
    start = std::chrono::steady_clock::now();
  }
}

TaskTraceBlock::~TaskTraceBlock() {
  if (threadId == 0) {
    return;
  }

  auto elapsed = std::chrono::duration_cast<std::chrono::microseconds>(
      std::chrono::steady_clock::now() - start);
  publish(TaskTraceEvent(
      name,
      threadName,
      threadId,
      elapsed,
      std::chrono::duration_cast<std::chrono::microseconds>(
          start.time_since_epoch())));
}
} // namespace facebook::eden
