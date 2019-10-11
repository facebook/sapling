/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/service/PeriodicTask.h"

#include <folly/Random.h>
#include <folly/io/async/EventBase.h>
#include <folly/lang/Bits.h>
#include <folly/stop_watch.h>

#include "eden/fs/service/EdenServer.h"

using namespace std::chrono_literals;

namespace {
constexpr auto kSlowTaskLimit = 50ms;
}

namespace facebook {
namespace eden {

PeriodicTask::PeriodicTask(EdenServer* server, folly::StringPiece name)
    : server_{server}, name_{name.str()}, interval_{0} {}

void PeriodicTask::timeoutExpired() noexcept {
  folly::stop_watch<> timer;
  try {
    running_ = true;
    runTask();
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error running periodic task " << name_ << ": "
              << folly::exceptionStr(ex);
  }
  running_ = false;

  // Log a warning if any of the periodic tasks take longer than 50ms to run.
  // Since these run on the main EventBase thread we want to ensure that they
  // don't block this thread for long periods of time.
  auto duration = timer.elapsed();
  XLOG(DBG6) << "ran periodic task " << name_ << " in "
             << (std::chrono::duration_cast<std::chrono::microseconds>(duration)
                     .count() /
                 1000.0)
             << "ms";
  if (duration > kSlowTaskLimit) {
    // Just in case some task starts frequently running slowly for some reason,
    // put some rate limiting on this log message.
    // Using popcount() give us exponential backoff.
    ++slowCount_;
    if (folly::popcount(slowCount_) == 1) {
      XLOG(WARN) << "slow periodic task: " << name_ << " took "
                 << (std::chrono::duration_cast<std::chrono::microseconds>(
                         duration)
                         .count() /
                     1000.0)
                 << "ms; has run slowly " << slowCount_ << " times";
    }
  }

  reschedule();
}

void PeriodicTask::updateInterval(Duration interval, bool splay) {
  server_->getMainEventBase()->dcheckIsInEventBaseThread();

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
  server_->getMainEventBase()->timer().scheduleTimeout(
      this, initialScheduleTime);
}

void PeriodicTask::reschedule() {
  if (interval_ <= Duration(0)) {
    return;
  }
  server_->getMainEventBase()->timer().scheduleTimeout(this, interval_);
}

} // namespace eden
} // namespace facebook
