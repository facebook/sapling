/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include "eden/fs/telemetry/TraceBus.h"

#include <chrono>
#include <string>
#include <string_view>

namespace facebook::eden {

struct TaskTraceEvent : TraceEventBase {
  std::string_view name;
  std::string threadName;
  uint64_t threadId;
  std::chrono::microseconds duration;
  std::chrono::microseconds start;

  TaskTraceEvent(
      std::string_view name,
      std::string threadName,
      uint64_t threadId,
      std::chrono::microseconds duration,
      std::chrono::microseconds start)
      : name(name),
        threadName(std::move(threadName)),
        threadId(threadId),
        duration(duration),
        start(start) {}

  static const std::shared_ptr<TraceBus<TaskTraceEvent>>& getTraceBus();
};

struct TaskTraceBlock {
  std::string_view name;
  std::string threadName;
  uint64_t threadId{0};
  std::chrono::steady_clock::time_point start;

  template <size_t size>
  explicit TaskTraceBlock(const char (&name)[size])
      : TaskTraceBlock(std::string_view{name}) {}

  // It's difficult to trace across blocks, disabling both move and copy to
  // prevent accidental incorrect usage.
  TaskTraceBlock(const TaskTraceBlock&) = delete;
  TaskTraceBlock& operator=(const TaskTraceBlock&) = delete;

  TaskTraceBlock(TaskTraceBlock&&) = delete;
  TaskTraceBlock& operator=(TaskTraceBlock&&) = delete;

  ~TaskTraceBlock();

 private:
  explicit TaskTraceBlock(std::string_view name);
};
} // namespace facebook::eden
