/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/PeriodicTask.h"

#include <folly/Random.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <folly/stop_watch.h>

namespace facebook::eden {

PeriodicTask::PeriodicTask(folly::EventBase* evb, std::string name)
    : evb_{evb}, name_{std::move(name)}, interval_{0} {}

void PeriodicTask::timeoutExpired() noexcept {
  folly::stop_watch<> timer;
  try {
    running_ = true;
    runTask();
  } catch (const std::exception& ex) {
    XLOGF(
        ERR,
        "error running periodic task {}: {}",
        name_,
        folly::exceptionStr(ex));
  }
  running_ = false;

  // Log a warning if any of the periodic tasks take longer than 50ms to run.
  // Since these run on the main EventBase thread we want to ensure that they
  // don't block this thread for long periods of time.
  auto duration = timer.elapsed();
  XLOGF(
      DBG6,
      "ran periodic task {} in {}ms",
      name_,
      (std::chrono::duration_cast<std::chrono::microseconds>(duration).count() /
       1000.0));
  if (duration > runDurationThreshold_) {
    // Just in case some task starts frequently running slowly for some reason,
    // put some rate limiting on this log message.
    // Using popcount() give us exponential backoff.
    ++slowCount_;
    if (folly::popcount(slowCount_) == 1) {
      XLOGF(
          WARN,
          "slow periodic task: {} took {}ms; has run slowly {} times",
          name_,
          (std::chrono::duration_cast<std::chrono::microseconds>(duration)
               .count() /
           1000.0),
          slowCount_);
    }
  }

  reschedule();
}

void PeriodicTask::updateInterval(
    Duration interval,
    std::chrono::milliseconds runDurationThreshold,
    bool splay) {
  evb_->dcheckIsInEventBaseThread();
  runDurationThreshold_ = runDurationThreshold;

  auto oldInterval = interval_;
  interval_ = interval;
  if (running_) {
    // reschedule() will handle rescheduling us as appropriate
    return;
  }

  if (interval_ <= Duration(0)) {
    cancelTimeout();
    return;
  }

  if (isScheduled() && oldInterval == interval_) {
    return;
  }

  auto initialScheduleTime = interval_;
  if (splay && !isScheduled()) {
    initialScheduleTime += Duration(folly::Random::rand64(interval_.count()));
  }
  cancelTimeout();
  evb_->timer().scheduleTimeout(this, initialScheduleTime);
}

void PeriodicTask::reschedule() {
  if (interval_ <= Duration(0)) {
    cancelTimeout(); // no need to reschedule
    return;
  }
  evb_->timer().scheduleTimeout(this, interval_);
}

} // namespace facebook::eden
