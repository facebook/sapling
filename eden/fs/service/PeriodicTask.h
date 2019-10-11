/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <chrono>
#include <string>

#include <folly/Range.h>
#include <folly/io/async/HHWheelTimer.h>

namespace facebook {
namespace eden {

class EdenServer;

/**
 * A helper class for implementing periodic tasks that should be run by
 * EdenServer.
 *
 * Tasks will run on the main EventBase thread.  As a result tasks should
 * complete relatively quickly.  If a task needs to perform an expensive
 * operation it should schedule it in a separate executor.
 */
class PeriodicTask : private folly::HHWheelTimer::Callback {
 public:
  // This should match the duration used by folly::HHWheelTimer.
  // Unfortunately HHWheelTimer does not expose this as a class member.
  using Duration = std::chrono::milliseconds;

  PeriodicTask(EdenServer* server, folly::StringPiece name);

  EdenServer* getServer() const {
    return server_;
  }

  const std::string& getName() const {
    return name_;
  }

  /**
   * Get the interval on which this task is scheduled.
   *
   * This function should only be called from the EdenServer's main event base
   * thread.
   */
  Duration getInterval() const {
    return interval_;
  }

  /**
   * Update the interval at which the PeriodicTask runs.
   *
   * If interval is 0 or negative the task will be stopped, otherwise the task
   * will be scheduled to run at the specified interval.
   *
   * If the task was not previously running and splay is true, a random amount
   * of time between 0 and interval will be added before the task runs for the
   * first time.  Therefore the first run won't happen until somewhere between
   * [interval and 2*interval].  If you have multiple tasks running with the
   * same interval this helps distribute tasks out along the interval, rather
   * than having them all try to run at the same time at the start of each
   * interval period.  If the task was already running the splay parameter is
   * ignored.
   */
  void updateInterval(Duration interval, bool splay = true);

 protected:
  /**
   * Subclasses should implement runTask()
   */
  virtual void runTask() = 0;

 private:
  /**
   * Implementation of the HHWheelTimer::Callback interface.
   */
  void timeoutExpired() noexcept override final;

  void reschedule();

  EdenServer* const server_;
  std::string const name_;

  /*
   * PeriodicTask objects are only ever used from the EdenServer's main
   * EventBase thread.  Therefore we do not need synchronization for accessing
   * the mutable member variables.
   */

  /**
   * How frequently this PeriodicTask should be scheduled.
   */
  Duration interval_;

  /**
   * The number of times this task has run slowly.
   * This is tracked purely for reporting purposes.
   */
  size_t slowCount_{0};

  /**
   * running_ is set to true while runTask() is running.
   */
  bool running_{false};
};

} // namespace eden
} // namespace facebook
