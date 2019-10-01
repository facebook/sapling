/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/FutureSubprocess.h"
#include <folly/executors/GlobalExecutor.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/EventBaseManager.h>

namespace facebook {
namespace eden {

namespace {

/** SubprocessTimeout polls the status of a folly::Subprocess
 * every poll_interval milliseconds.
 * When the process stops running it will fulfil a Promise
 * with the ProcessReturnCode.
 */
class SubprocessTimeout : public folly::AsyncTimeout {
 public:
  SubprocessTimeout(
      folly::EventBase* event_base,
      folly::Subprocess proc,
      std::chrono::milliseconds poll_interval)
      : AsyncTimeout(event_base),
        pollEveryMs_(poll_interval),
        subprocess_(std::move(proc)) {}

  folly::SemiFuture<folly::ProcessReturnCode> initialize() {
    auto future = returnCode_.getSemiFuture();
    scheduleTimeout(pollEveryMs_.count());
    return future;
  }

  void timeoutExpired() noexcept override {
    auto ret = subprocess_.poll();
    if (UNLIKELY(!ret.running())) {
      returnCode_.setValue(std::move(ret));
      delete this;
      return;
    }
    scheduleTimeout(pollEveryMs_.count());
  }

 private:
  const std::chrono::milliseconds pollEveryMs_;
  folly::Subprocess subprocess_;
  folly::Promise<folly::ProcessReturnCode> returnCode_;
};

} // namespace

folly::SemiFuture<folly::ProcessReturnCode> futureSubprocess(
    folly::Subprocess proc,
    std::chrono::milliseconds poll_interval) {
  // We need to be running in a thread with an eventBase, so switch
  // over to the IOExecutor eventbase
  return folly::via(
             folly::getIOExecutor().get(),
             [process = std::move(proc), poll_interval]() mutable {
               // Create a self-owned SubprocessTimeout instance and start
               // the timer.
               return (new SubprocessTimeout(
                           folly::EventBaseManager::get()->getEventBase(),
                           std::move(process),
                           poll_interval))
                   ->initialize();
             })
      .semi();
}

} // namespace eden
} // namespace facebook
