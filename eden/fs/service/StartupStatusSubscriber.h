/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <list>
#include <memory>
#include <string_view>

#include <folly/Synchronized.h>

namespace facebook::eden {

/**
 * Some one who wants to be informed of startup status updates should inherit
 * from this.
 */
class StartupStatusSubscriber {
 public:
  /**
   * The subscriber will be destroyed when startup completes (error or no error
   * ), or if the subscriber can not be added to the
   * StartupStatusSubscriberState (this would most notably happen if startup
   * has already completed but might also happen if there are locking or
   * allocation errors thrown thrown).
   */
  virtual ~StartupStatusSubscriber() noexcept = default;

  /**
   * Called to publish a bit of startup status. Be careful of blocking
   * operations here they will block startup.
   *
   * StartupStatusChannel holds an internal lock while this is called, so do not
   * call any StartupStatusChannel methods from this callback!! You will
   * deadlock!!
   *
   * Admitadly, with the current StartupStatusSubscriber implementation, publish
   * will not be invoked more than once at a time. However, this is considered
   * an implementation detail that might change, so it is safer ensure that
   * publish can be called concurrently with itself.
   */
  virtual void publish(std::string_view data) = 0;
};

/**
 * State that tracks where to publish startup status to. This will be shared
 * by the EdenServer (to allow thrift clients to subscribe to startup status)
 * and StartupLogger which produces startup status.
 *
 * This class is thread safe. all methods may be called from multiple threads
 * at any time.
 */
class StartupStatusChannel {
 public:
  /**
   * If startup has not yet completed, this adds the subscriber to the
   * subscription list, and all future publishes will be forwarded to this
   * subscriber.
   *
   * May thrown on allocation or locking errors.
   */
  void subscribe(std::unique_ptr<StartupStatusSubscriber> subscriber);

  /**
   * publishes some a startup status update to all subscribers.
   *
   * This will call the subscriber publish method inline for each subscriber.
   * This means expensive subscribers could block startup progress.
   */
  void publish(std::string_view data);

  /**
   * Clears all publishers from the subscription list. subscribers will be
   * destroyed inline.
   */
  void startupCompleted();

 private:
  struct StartupStatusChannelInner {
    bool subscribersClosed = false;
    std::vector<std::unique_ptr<StartupStatusSubscriber>> subscribers;
  };

  folly::Synchronized<StartupStatusChannelInner, std::mutex> state_;
};
} // namespace facebook::eden
