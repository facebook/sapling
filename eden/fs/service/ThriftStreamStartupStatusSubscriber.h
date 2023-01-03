/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <atomic>
#include <memory>
#include <string>
#include <string_view>

#include <folly/CancellationToken.h>
#include <thrift/lib/cpp2/async/ServerPublisherStream.h>
#include <thrift/lib/cpp2/async/ServerStream.h>

#include "eden/fs/service/StartupStatusSubscriber.h"

namespace facebook::eden {

class ThriftStreamStartupStatusSubscriber : public StartupStatusSubscriber {
 public:
  ThriftStreamStartupStatusSubscriber(
      apache::thrift::ServerStreamPublisher<std::string> publisher,
      folly::CancellationToken cancellationToken);

  ~ThriftStreamStartupStatusSubscriber() noexcept override;

  /**
   * publishes a startup status update to a Thrift stream as long as the stream
   * has not yet been cancled. Publishing to the Thrift stream is done inline,
   * so this will block if there is back pressure from thrift.
   */
  void publish(std::string_view data) override;

  /**
   * This creates a new Thrift publisher stream pair and subscribes the
   * publisher to the StartupStatusSubscriberState.
   *
   * Throws an EdenError when startup has already completed and various errors
   * if there are problems creating the pair or adding the publisher to
   * StartupStatusSubscriberState.
   */
  static apache::thrift::ServerStream<std::string>
  createStartupStatusThriftStream(
      std::shared_ptr<StartupStatusChannel>& startupStatusSubscribers);

 private:
  // Has the publisher been canceled or completed already. This prevents
  // publishing errors to the underlying thrift publisher after its been torn
  // down which will raise null pointer segfaults.
  folly::CancellationToken cancellationToken_;

  // The actual Thrift stream publisher.
  apache::thrift::ServerStreamPublisher<std::string> publisher_;
};
} // namespace facebook::eden
